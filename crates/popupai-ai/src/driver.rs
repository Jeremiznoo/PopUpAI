//! HTTP plumbing shared by every adapter.
//!
//! Security properties enforced here, once, for all providers:
//! - redirects are never followed, so a credential cannot be forwarded to
//!   another host;
//! - API keys travel in headers marked sensitive, never in URLs;
//! - error bodies are size-limited and redacted before they become messages;
//! - dropping the returned stream drops the response, which aborts the request.

use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt, stream};
use reqwest::header::{HeaderValue, RETRY_AFTER};
use reqwest::{Client, RequestBuilder, Response, redirect};
use serde_json::Value;

use crate::error::{error_message, from_reqwest};
use crate::{ChatStream, ProviderError};

pub(crate) type Out = VecDeque<Result<String, ProviderError>>;

/// Turns raw response bytes into text pieces for one provider's wire format.
pub(crate) trait ChunkParser: Send {
    fn feed(&mut self, bytes: &[u8], out: &mut Out);
    /// The connection ended. Report an error here if the provider's completion
    /// marker was expected but never arrived.
    fn finish(&mut self, out: &mut Out);
    /// The provider signalled the end (or a fatal error); stop reading.
    fn is_done(&self) -> bool;
}

const ERROR_BODY_LIMIT: usize = 8 * 1024;
const JSON_BODY_LIMIT: usize = 4 * 1024 * 1024;

/// One client per provider. `timeout` bounds connecting and each wait for more
/// data, not the whole answer, so long generations are not cut off.
pub(crate) fn build_client(timeout: Duration) -> Result<Client, ProviderError> {
    Client::builder()
        .redirect(redirect::Policy::none())
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .user_agent(concat!("PopUpAI/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(from_reqwest)
}

/// A header value carrying a secret, flagged so HTTP debug output hides it.
pub(crate) fn secret_header(value: &str) -> Result<HeaderValue, ProviderError> {
    let mut header = HeaderValue::from_str(value).map_err(|_| {
        ProviderError::InvalidRequest(
            "the API key contains characters that are not valid in an HTTP header".into(),
        )
    })?;
    header.set_sensitive(true);
    Ok(header)
}

/// Send a request and map every non-success outcome to a [`ProviderError`].
pub(crate) async fn send(request: RequestBuilder) -> Result<Response, ProviderError> {
    let response = request.send().await.map_err(from_reqwest)?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status.is_redirection() {
        return Err(ProviderError::RedirectRefused {
            status: status.as_u16(),
        });
    }
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    let message = error_message(&read_error_body(response).await);
    Err(match status.as_u16() {
        code @ (401 | 403) => ProviderError::Auth {
            status: code,
            message,
        },
        429 => ProviderError::RateLimited { retry_after },
        code => ProviderError::Api {
            status: code,
            message,
        },
    })
}

async fn read_error_body(mut response: Response) -> String {
    let mut body = Vec::new();
    while body.len() < ERROR_BODY_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

pub(crate) async fn read_json(mut response: Response) -> Result<Value, ProviderError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(from_reqwest)? {
        body.extend_from_slice(&chunk);
        if body.len() > JSON_BODY_LIMIT {
            return Err(ProviderError::Decode("the response is too large".into()));
        }
    }
    serde_json::from_slice(&body)
        .map_err(|_| ProviderError::Decode("the response was not valid JSON".into()))
}

struct Pump<P> {
    bytes: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    parser: P,
    pending: Out,
    finished: bool,
}

/// Drive a response through a parser as a stream of text pieces. The first
/// error ends the stream.
pub(crate) fn into_stream<P: ChunkParser + 'static>(response: Response, parser: P) -> ChatStream {
    let pump = Pump {
        bytes: Box::pin(response.bytes_stream()),
        parser,
        pending: VecDeque::new(),
        finished: false,
    };
    Box::pin(stream::unfold(pump, |mut p| async move {
        loop {
            if let Some(item) = p.pending.pop_front() {
                if item.is_err() {
                    p.pending.clear();
                    p.finished = true;
                }
                return Some((item, p));
            }
            if p.finished {
                return None;
            }
            if p.parser.is_done() {
                p.finished = true;
                continue;
            }
            match p.bytes.next().await {
                Some(Ok(chunk)) => p.parser.feed(&chunk, &mut p.pending),
                Some(Err(err)) => p.pending.push_back(Err(from_reqwest(err))),
                None => {
                    p.parser.finish(&mut p.pending);
                    p.finished = true;
                }
            }
        }
    }))
}

/// Feed chunks to a parser the way [`into_stream`] does, for unit tests.
#[cfg(test)]
pub(crate) fn run_parser(
    parser: &mut impl ChunkParser,
    chunks: &[&[u8]],
) -> Vec<Result<String, ProviderError>> {
    let mut out = Out::new();
    for chunk in chunks {
        if parser.is_done() {
            break;
        }
        parser.feed(chunk, &mut out);
    }
    if !parser.is_done() {
        parser.finish(&mut out);
    }
    out.into_iter().collect()
}
