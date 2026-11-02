use std::fmt;
use std::pin::Pin;

use futures_util::Stream;

use crate::ProviderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    /// The name most chat APIs use.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Prompts are private: `Debug` shows the size, never the text.
impl fmt::Debug for ChatMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ChatMessage({:?}, {} bytes)",
            self.role,
            self.content.len()
        )
    }
}

#[derive(Clone, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl fmt::Debug for ChatRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatRequest")
            .field("model", &self.model)
            .field("messages", &self.messages.len())
            .field("has_system_prompt", &self.system_prompt.is_some())
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl ChatRequest {
    /// Check the request before any network call. A model identifier is free
    /// text, but it must be safe to place in a URL path or a JSON body.
    pub(crate) fn validate(&self) -> Result<(), ProviderError> {
        let model_ok = !self.model.is_empty()
            && self.model.len() <= 200
            && self.model.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-')
            });
        if !model_ok {
            return Err(ProviderError::InvalidRequest(
                "the model identifier is empty or contains unsupported characters".into(),
            ));
        }
        if self.messages.is_empty() {
            return Err(ProviderError::InvalidRequest(
                "there is no message to send".into(),
            ));
        }
        if let Some(t) = self.temperature
            && !(t.is_finite() && (0.0..=2.0).contains(&t))
        {
            return Err(ProviderError::InvalidRequest(
                "the temperature must be between 0 and 2".into(),
            ));
        }
        if self.max_tokens == Some(0) {
            return Err(ProviderError::InvalidRequest(
                "the maximum output length must be at least 1 token".into(),
            ));
        }
        Ok(())
    }

    /// Temperature rounded to two decimals, so `0.7f32` serialises as `0.7`
    /// and not `0.699999988079071`.
    pub(crate) fn temperature_json(&self) -> Option<f64> {
        self.temperature
            .map(|t| (f64::from(t) * 100.0).round() / 100.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelInfo {
    pub id: String,
}

/// Text pieces as they arrive. Dropping the stream aborts the HTTP request.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage::user("a very private question")],
            system_prompt: Some("a private system prompt".into()),
            ..Default::default()
        }
    }

    #[test]
    fn debug_output_never_contains_prompt_text() {
        let req = request();
        let shown = format!("{req:?} {:?}", req.messages[0]);
        assert!(!shown.contains("private"), "{shown}");
        assert!(shown.contains("gpt-4o-mini"));
    }

    #[test]
    fn validate_accepts_typical_model_ids() {
        for id in [
            "gpt-4o-mini",
            "claude-sonnet-5-5",
            "gemini-2.0-flash",
            "llama3.2:latest",
            "anthropic/claude-3.5-sonnet:beta",
            "hf.co/org/model:Q4_K_M",
        ] {
            let req = ChatRequest {
                model: id.into(),
                ..request()
            };
            assert!(req.validate().is_ok(), "{id}");
        }
    }

    #[test]
    fn validate_rejects_unsafe_or_missing_values() {
        let bad_model = ChatRequest {
            model: "x?key=1 y".into(),
            ..request()
        };
        assert!(bad_model.validate().is_err());
        assert!(
            ChatRequest {
                model: String::new(),
                ..request()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatRequest {
                messages: vec![],
                ..request()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatRequest {
                temperature: Some(f32::NAN),
                ..request()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatRequest {
                temperature: Some(2.5),
                ..request()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatRequest {
                max_tokens: Some(0),
                ..request()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatRequest {
                temperature: Some(0.0),
                max_tokens: Some(1),
                ..request()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn temperature_serialises_without_float_noise() {
        let req = ChatRequest {
            temperature: Some(0.7),
            ..request()
        };
        assert_eq!(req.temperature_json(), Some(0.7));
        assert_eq!(request().temperature_json(), None);
    }
}
