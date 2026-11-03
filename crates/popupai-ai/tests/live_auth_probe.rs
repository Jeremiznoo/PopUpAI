//! One request to each hosted vendor with a deliberately **invalid** key.
//!
//! It proves, against the real services, what does not depend on a valid key: that the
//! endpoint paths and the authentication headers are the ones each vendor expects (an
//! authentication error comes back, not "not found"), that the failure is classified as an
//! authentication problem, and that the invalid key never appears in the message shown to
//! the user. It cannot show that a *valid* key works: that needs the user's own key.
//!
//! Ignored by default because it uses the network. The only things sent are a fixed test
//! sentence and a made-up key.
//!
//! ```text
//! cargo test -p popupai-ai --test live_auth_probe -- --ignored --nocapture
//! ```

use popupai_ai::{
    AnthropicProvider, ChatMessage, ChatRequest, GeminiProvider, OpenAiFlavor, OpenAiProvider,
    Provider, ProviderConfig, ProviderError,
};
use popupai_security::{Endpoint, HttpPolicy, Secret};

const FAKE_KEY: &str = "invalid-key-for-an-endpoint-check-0123456789";

fn config(endpoint: &str) -> ProviderConfig {
    ProviderConfig::new(Endpoint::parse(endpoint, HttpPolicy::default()).unwrap())
        .with_api_key(Secret::new(FAKE_KEY))
}

fn request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        messages: vec![ChatMessage::user("ping")],
        max_tokens: Some(8),
        ..Default::default()
    }
}

async fn probe(name: &str, provider: &dyn Provider, model: &str) {
    let outcome = provider.stream_chat(request(model)).await;
    let Err(error) = outcome else {
        panic!("{name}: a made-up key was accepted, which cannot be right");
    };
    let shown = error.to_string();
    println!("{name}: {shown}");
    assert!(
        matches!(error, ProviderError::Auth { .. }),
        "{name}: expected an authentication error, got {error:?}"
    );
    assert!(
        !shown.contains(FAKE_KEY),
        "{name}: the key leaked into the message"
    );
}

#[tokio::test]
#[ignore = "sends one request with an invalid key to each hosted vendor"]
async fn each_hosted_vendor_rejects_an_invalid_key_as_an_authentication_error() {
    let anthropic = AnthropicProvider::new(config("https://api.anthropic.com/v1")).unwrap();
    probe("anthropic", &anthropic, "claude-haiku-5-5").await;

    let openai =
        OpenAiProvider::new(config("https://api.openai.com/v1"), OpenAiFlavor::OpenAi).unwrap();
    probe("openai", &openai, "gpt-6-luna").await;

    let deepseek =
        OpenAiProvider::new(config("https://api.deepseek.com"), OpenAiFlavor::Compatible).unwrap();
    probe("deepseek", &deepseek, "deepseek-flash").await;

    let gemini =
        GeminiProvider::new(config("https://generativelanguage.googleapis.com/v1beta")).unwrap();
    probe("gemini", &gemini, "gemini-3.5-flash-lite").await;
}
