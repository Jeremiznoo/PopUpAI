//! A tiny scripted HTTP server on the loopback interface, so the adapters can be
//! tested over real sockets without any external network or extra dependency.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

/// What the server received for one request.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    /// Request target: path plus query string.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("request body is JSON")
    }
}

pub enum Reply {
    /// A complete response with a fixed body.
    Fixed {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    /// A body of unknown length, written piece by piece, then the connection
    /// closes cleanly.
    Stream { chunks: Vec<(Duration, String)> },
    /// The same piece every `interval`, until the client disconnects.
    Endless { chunk: String, interval: Duration },
    /// Read the request and never answer.
    Silent,
}

impl Reply {
    pub fn json(body: &str) -> Self {
        Self::status(200, body)
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self::Fixed {
            status,
            headers: vec![("content-type", "application/json".into())],
            body: body.into(),
        }
    }

    pub fn stream(chunks: &[&str]) -> Self {
        Self::Stream {
            chunks: chunks
                .iter()
                .map(|c| (Duration::from_millis(5), (*c).to_owned()))
                .collect(),
        }
    }
}

type Handler = dyn Fn(&Recorded) -> Reply + Send + Sync;

pub struct Mock {
    pub addr: SocketAddr,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    disconnects: Arc<AtomicUsize>,
}

impl Mock {
    pub async fn start(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let disconnects = Arc::new(AtomicUsize::new(0));
        let handler: Arc<Handler> = Arc::new(handler);
        {
            let (recorded, disconnects) = (recorded.clone(), disconnects.clone());
            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    tokio::spawn(serve(
                        socket,
                        recorded.clone(),
                        disconnects.clone(),
                        handler.clone(),
                    ));
                }
            });
        }
        Self {
            addr,
            recorded,
            disconnects,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().unwrap().clone()
    }

    /// How many times a client went away while the server was still talking.
    pub fn disconnects(&self) -> usize {
        self.disconnects.load(Ordering::SeqCst)
    }
}

async fn serve(
    socket: TcpStream,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    disconnects: Arc<AtomicUsize>,
    handler: Arc<Handler>,
) {
    let (mut reader, mut writer) = socket.into_split();
    let Some(request) = read_request(&mut reader).await else {
        return;
    };
    let reply = handler(&request);
    recorded.lock().unwrap().push(request);

    let gone = || {
        disconnects.fetch_add(1, Ordering::SeqCst);
    };
    match reply {
        Reply::Fixed {
            status,
            headers,
            body,
        } => {
            let mut head = format!("HTTP/1.1 {status} Reply\r\nconnection: close\r\n");
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
            let _ = writer.write_all(head.as_bytes()).await;
            let _ = writer.write_all(body.as_bytes()).await;
            let _ = writer.shutdown().await;
        }
        Reply::Stream { chunks } => {
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            if writer.write_all(head.as_bytes()).await.is_err() {
                return gone();
            }
            for (delay, chunk) in chunks {
                if client_left_within(&mut reader, delay).await
                    || writer.write_all(chunk.as_bytes()).await.is_err()
                {
                    return gone();
                }
            }
            let _ = writer.shutdown().await;
        }
        Reply::Endless { chunk, interval } => {
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            if writer.write_all(head.as_bytes()).await.is_err() {
                return gone();
            }
            loop {
                if client_left_within(&mut reader, interval).await
                    || writer.write_all(chunk.as_bytes()).await.is_err()
                {
                    return gone();
                }
            }
        }
        Reply::Silent => {
            if client_left_within(&mut reader, Duration::from_secs(10)).await {
                gone();
            }
        }
    }
}

/// Wait up to `delay`; report whether the client closed the connection meanwhile.
async fn client_left_within(reader: &mut OwnedReadHalf, delay: Duration) -> bool {
    let mut buf = [0u8; 64];
    tokio::select! {
        () = sleep(delay) => false,
        n = reader.read(&mut buf) => matches!(n, Ok(0) | Err(_)),
    }
}

async fn read_request(reader: &mut OwnedReadHalf) -> Option<Recorded> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = reader.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < length {
        let n = reader.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some(Recorded {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body[..length]).into_owned(),
    })
}
