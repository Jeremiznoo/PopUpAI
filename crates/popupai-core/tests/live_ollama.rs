//! The whole core (configuration defaults, orchestrator, real adapter) against a **real**
//! Ollama server on this machine. Ignored by default; see `popupai-ai`'s `live_ollama`.
//!
//! ```text
//! cargo test -p popupai-core --test live_ollama -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::Duration;

use popupai_core::config::Config;
use popupai_core::orchestrator::{DefaultProviderFactory, Event, Orchestrator};
use popupai_security::MemoryStore;
use tokio::time::timeout;

#[tokio::test]
#[ignore = "needs a real Ollama server with a model installed"]
async fn the_first_run_configuration_answers_through_a_real_ollama() {
    // The first-run configuration, with Ollama chosen because no key is stored.
    let (orchestrator, mut events) = Orchestrator::new(
        Config::builtin(),
        Arc::new(MemoryStore::default()),
        Arc::new(DefaultProviderFactory),
    )
    .expect("the built-in configuration is valid");
    orchestrator.auto_select().await;
    assert_eq!(orchestrator.current_provider().unwrap().id, "ollama");
    assert!(!orchestrator.current_needs_key().await);

    orchestrator.open();
    orchestrator
        .submit("In one short sentence, what is DNS?")
        .expect("the question is accepted");

    let mut answer = String::new();
    loop {
        let event = timeout(Duration::from_secs(60), events.recv())
            .await
            .expect("the answer arrives in time")
            .expect("the event channel stays open");
        match event {
            Event::Chunk { text, .. } => answer.push_str(&text),
            Event::Finished { .. } => break,
            Event::Failed { message, .. } => panic!("the request failed: {message}"),
            Event::Permission { .. } => panic!("no command is expected from a local model here"),
        }
    }
    println!("answer: {answer:?}");
    assert!(answer.trim().len() > 10, "a real sentence came back");
}
