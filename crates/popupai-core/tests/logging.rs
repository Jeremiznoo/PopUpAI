//! Privacy regression test: whatever happens during a request, the logs must
//! never contain prompts, answers, error text from a server, or API keys.
//!
//! This is its own test binary with a single test on purpose. `tracing` caches
//! which log points are enabled process-wide, so running next to other tests
//! that have no subscriber can hide events and make the test pass or fail for
//! the wrong reason.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::stream;
use popupai_ai::{ChatRequest, ChatStream, ModelInfo, Provider, ProviderError};
use popupai_core::config::{Config, ProviderEntry, ProviderKind};
use popupai_core::orchestrator::{Event, Orchestrator, ProviderFactory};
use popupai_security::{Endpoint, MemoryStore, Secret};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::timeout;
use tracing_subscriber::fmt::MakeWriter;

type Feed = UnboundedReceiver<Result<String, ProviderError>>;

#[derive(Default)]
struct Scripted {
    feeds: Mutex<VecDeque<Feed>>,
}

impl Provider for Scripted {
    fn kind(&self) -> &'static str {
        "scripted"
    }

    fn stream_chat(
        &self,
        _request: ChatRequest,
    ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
        let feed = self.feeds.lock().unwrap().pop_front();
        Box::pin(async move {
            let stream = stream::unfold(feed, |feed| async move {
                let mut rx = feed?;
                let item = rx.recv().await?;
                Some((item, Some(rx)))
            });
            Ok(Box::pin(stream) as ChatStream)
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct Factory(Arc<Scripted>);

impl ProviderFactory for Factory {
    fn build(
        &self,
        _entry: &ProviderEntry,
        _endpoint: Endpoint,
        _key: Option<Secret>,
        _timeout: Duration,
    ) -> Result<Arc<dyn Provider>, ProviderError> {
        Ok(self.0.clone())
    }
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> LogBuffer {
        self.clone()
    }
}

fn config(model: &str) -> Config {
    let mut provider = ProviderEntry::new("fake", ProviderKind::OpenAiCompatible);
    provider.endpoint = Some("http://127.0.0.1:9/v1".into());
    provider.model = model.into();
    Config {
        providers: vec![provider],
        ..Config::default()
    }
}

async fn next(events: &mut UnboundedReceiver<Event>) -> Event {
    timeout(Duration::from_secs(3), events.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("the event channel closed")
}

async fn until(events: &mut UnboundedReceiver<Event>, wanted: fn(&Event) -> bool) {
    while !wanted(&next(events).await) {}
}

fn feed(provider: &Scripted) -> UnboundedSender<Result<String, ProviderError>> {
    let (tx, rx) = unbounded_channel();
    provider.feeds.lock().unwrap().push_back(rx);
    tx
}

#[tokio::test]
async fn logs_never_contain_prompts_answers_server_text_or_keys() {
    let log = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    const KEY: &str = "KEY-SECRET-0123456789";
    let provider = Arc::new(Scripted::default());
    let store = Arc::new(MemoryStore::default());
    let (orch, mut events) = Orchestrator::new(
        config("test-model"),
        store,
        Arc::new(Factory(provider.clone())),
    )
    .unwrap();
    orch.save_api_key("fake", Secret::new(KEY)).await.unwrap();

    // 1. A successful request.
    let answer = feed(&provider);
    orch.open();
    orch.submit("PRIVATE-PROMPT-ONE").unwrap();
    answer.send(Ok("PRIVATE-ANSWER-ONE".into())).unwrap();
    drop(answer);
    until(&mut events, |e| matches!(e, Event::Finished { .. })).await;

    // 2. A failing request whose error text echoes the key.
    let failing = feed(&provider);
    orch.submit("PRIVATE-PROMPT-TWO").unwrap();
    failing
        .send(Err(ProviderError::Api {
            status: 400,
            message: format!("PRIVATE-ERROR-TEXT {KEY}"),
        }))
        .unwrap();
    until(&mut events, |e| matches!(e, Event::Failed { .. })).await;

    // 3. A request that is stopped halfway.
    let stopped = feed(&provider);
    orch.submit("PRIVATE-PROMPT-THREE").unwrap();
    stopped.send(Ok("PRIVATE-ANSWER-THREE".into())).unwrap();
    until(&mut events, |e| matches!(e, Event::Chunk { .. })).await;
    orch.stop();
    timeout(Duration::from_secs(3), stopped.closed())
        .await
        .expect("the stopped request should have been dropped");

    // 4. A request that cannot even start.
    orch.dismiss();
    orch.set_config(config("")).unwrap();
    orch.open();
    orch.submit("PRIVATE-PROMPT-FOUR").unwrap();
    until(&mut events, |e| matches!(e, Event::Failed { .. })).await;

    let logged = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    for expected in [
        "request started",
        "request finished",
        "request failed",
        "request could not start",
    ] {
        assert!(
            logged.contains(expected),
            "the test must really capture logs; `{expected}` is missing:\n{logged}"
        );
    }
    for secret in [KEY, "PRIVATE-"] {
        assert!(
            !logged.contains(secret),
            "`{secret}` leaked into the logs:\n{logged}"
        );
    }
}
