//! PopUpAI provider layer.
//!
//! One [`Provider`] trait with four adapters: OpenAI (and any OpenAI-compatible
//! endpoint), Anthropic, Google Gemini and Ollama. Responses stream as text
//! pieces; dropping the stream cancels the HTTP request.
//!
//! Wire formats were implemented from the providers' public documentation and
//! are covered by offline fixtures and a local mock server. They have **not**
//! been exercised against the live services; see `docs/DEVELOPMENT.md`.

mod anthropic;
mod decode;
mod driver;
mod error;
mod gemini;
mod ollama;
mod openai;
mod provider;
mod types;

pub use anthropic::AnthropicProvider;
pub use error::ProviderError;
pub use gemini::GeminiProvider;
pub use ollama::OllamaProvider;
pub use openai::{OpenAiFlavor, OpenAiProvider};
pub use provider::{DEFAULT_TIMEOUT, Provider, ProviderConfig};
pub use types::{ChatMessage, ChatRequest, ChatStream, ModelInfo, Role};
