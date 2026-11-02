//! The Ollama adapter against a **real** Ollama server on this machine.
//!
//! Ignored by default: it needs a running server on 127.0.0.1:11434 with at least one chat
//! model installed, and it sends one short question to it (nothing leaves the machine).
//!
//! ```text
//! cargo test -p popupai-ai --test live_ollama -- --ignored --nocapture
//! ```

use futures_util::StreamExt;
use popupai_ai::{ChatMessage, ChatRequest, OllamaProvider, Provider, ProviderConfig};
use popupai_security::{Endpoint, HttpPolicy};

fn provider() -> OllamaProvider {
    let endpoint = Endpoint::parse("http://127.0.0.1:11434", HttpPolicy::default()).unwrap();
    OllamaProvider::new(ProviderConfig::new(endpoint)).unwrap()
}

#[tokio::test]
#[ignore = "needs a real Ollama server with a model installed"]
async fn a_real_ollama_lists_its_models_and_answers_with_its_smallest_one() {
    let provider = provider();

    let models = provider.list_models().await.expect("list the models");
    assert!(
        !models.is_empty(),
        "install a model first: ollama pull llama3.2:1b"
    );
    println!(
        "installed: {:?}",
        models.iter().map(|m| &m.id).collect::<Vec<_>>()
    );

    // No model named: the adapter picks the smallest installed chat model.
    let request = ChatRequest {
        model: String::new(),
        messages: vec![ChatMessage::user("Reply with exactly one word: pong")],
        max_tokens: Some(32),
        ..Default::default()
    };
    let mut stream = provider
        .stream_chat(request)
        .await
        .expect("start the stream");
    let mut answer = String::new();
    let mut chunks = 0;
    while let Some(piece) = stream.next().await {
        answer.push_str(&piece.expect("a chunk, not an error"));
        chunks += 1;
    }
    println!("answer ({chunks} chunks): {answer:?}");
    assert!(
        !answer.trim().is_empty(),
        "the model answered with something"
    );
    assert!(chunks >= 1);
}
