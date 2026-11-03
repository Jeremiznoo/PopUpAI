//! PopUpAI provider layer.
//!
//! One [`Provider`] trait with four adapters: OpenAI (and any OpenAI-compatible
//! endpoint), Anthropic, Google Gemini and Ollama. Responses stream as text
//! pieces; dropping the stream cancels the HTTP request.
//!
//! Wire formats were implemented from the providers' public documentation and
//! are covered by offline fixtures and a local mock server. They have **not**
//! been exercised against the live services; see `docs/DEVELOPMENT.md`.

mod agent;
mod anthropic;
mod chatgpt;
mod decode;
mod driver;
mod error;
mod gemini;
mod oauth;
mod ollama;
mod openai;
mod provider;
mod types;

pub use agent::{AgentEvent, AgentItem, AgentRequest, AgentStream, ToolCall, ToolSpec};
pub use anthropic::AnthropicProvider;
pub use chatgpt::ChatGptProvider;
pub use error::ProviderError;
pub use gemini::GeminiProvider;
pub use ollama::OllamaProvider;

/// Signing in with a ChatGPT account (see the module for its status and limits).
pub mod chatgpt_login {
    pub use crate::oauth::{
        CLIENT_ID, DEFAULT_ISSUER, DeviceCode, Poll, Tokens, account_id_from_jwt, exchange,
        http_client, poll, refresh, request_device_code,
    };
}
pub use openai::{OpenAiFlavor, OpenAiProvider};
pub use provider::{DEFAULT_TIMEOUT, Provider, ProviderConfig};
pub use types::{ChatMessage, ChatRequest, ChatStream, ModelInfo, Role};
