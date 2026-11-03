use std::time::Duration;

use popupai_security::{EndpointError, redact};
use serde_json::Value;

/// Why a provider call failed. Messages are written to be shown to the user and
/// are passed through [`redact`], so a key echoed by a server never reaches the
/// UI or the logs.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("authentication failed (HTTP {status}): {message}")]
    Auth { status: u16, message: String },
    #[error("no API key is configured for this provider")]
    MissingApiKey,
    #[error("rate limited by the provider")]
    RateLimited { retry_after: Option<Duration> },
    /// Nothing is listening, or the host could not be reached at all. For a
    /// local provider this usually means "the server is not running".
    #[error("could not reach the server")]
    Unreachable,
    #[error("network error: {0}")]
    Network(String),
    #[error("the request timed out")]
    Timeout,
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(#[from] EndpointError),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error(
        "the server answered with a redirect (HTTP {status}); redirects are never followed so credentials cannot be forwarded"
    )]
    RedirectRefused { status: u16 },
    #[error("provider error (HTTP {status}): {message}")]
    Api { status: u16, message: String },
    /// The provider reported a failure in the middle of a stream.
    #[error("the provider reported an error while streaming: {0}")]
    Stream(String),
    #[error("unexpected response: {0}")]
    Decode(String),
}

const MAX_MESSAGE_CHARS: usize = 300;

pub(crate) fn from_reqwest(err: reqwest::Error) -> ProviderError {
    let err = err.without_url();
    if err.is_timeout() {
        ProviderError::Timeout
    } else if err.is_connect() {
        ProviderError::Unreachable
    } else {
        ProviderError::Network(redact(&err.to_string()))
    }
}

/// Pull a short human message out of a provider's error body. Handles the
/// OpenAI/Anthropic/Gemini `{"error":{"message":..}}` shape, Ollama's
/// `{"error":".."}`, and falls back to the raw text.
pub(crate) fn error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| v.get("error").and_then(Value::as_str))
            .or_else(|| v.get("message").and_then(Value::as_str))
            .map(str::to_owned)
    });
    let text = parsed.unwrap_or_else(|| body.trim().to_owned());
    redact(&text).chars().take(MAX_MESSAGE_CHARS).collect()
}

/// Google answers a wrong API key with HTTP 400 (not 401 or 403), saying so in the
/// error's `details` (`reason: API_KEY_INVALID`) and in its message. That is an
/// authentication problem, and the user needs to hear "check your key", not "bad request".
pub(crate) fn rejects_the_key(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let by_reason = value
        .pointer("/error/details")
        .and_then(Value::as_array)
        .is_some_and(|details| {
            details
                .iter()
                .any(|d| d.get("reason").and_then(Value::as_str) == Some("API_KEY_INVALID"))
        });
    by_reason
        || value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|m| m.to_ascii_lowercase().contains("api key not valid"))
}

/// An `error` member inside a streamed JSON event, if there is one.
pub(crate) fn stream_error(event: &Value) -> Option<ProviderError> {
    event
        .get("error")
        .map(|_| ProviderError::Stream(error_message(&event.to_string())))
}

/// The connection ended before the provider said the answer was complete.
pub(crate) fn truncated() -> ProviderError {
    ProviderError::Network("the connection closed before the response was complete".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_google_invalid_key_answer_is_recognised_by_reason_or_by_message() {
        let with_details = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID"}]}}"#;
        assert!(rejects_the_key(with_details));
        let message_only =
            r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key."}}"#;
        assert!(rejects_the_key(message_only));
        for other in [
            r#"{"error":{"code":400,"message":"Unknown name \"foo\": Cannot find field."}}"#,
            r#"{"error":{"details":[{"reason":"RATE_LIMIT_EXCEEDED"}]}}"#,
            "not json",
            "",
        ] {
            assert!(!rejects_the_key(other), "{other}");
        }
    }

    #[test]
    fn finds_errors_inside_stream_events() {
        let event: Value =
            serde_json::from_str(r#"{"type":"error","error":{"message":"overloaded"}}"#).unwrap();
        assert!(
            matches!(stream_error(&event), Some(ProviderError::Stream(m)) if m == "overloaded")
        );
        let ok: Value = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        assert!(stream_error(&ok).is_none());
    }

    #[test]
    fn extracts_messages_from_known_error_shapes() {
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key","type":"x"}}"#),
            "bad key"
        );
        assert_eq!(
            error_message(r#"{"error":"model not found"}"#),
            "model not found"
        );
        assert_eq!(error_message(r#"{"message":"nope"}"#), "nope");
        assert_eq!(error_message("plain text failure\n"), "plain text failure");
        assert_eq!(error_message(""), "");
    }

    #[test]
    fn error_messages_are_redacted_and_bounded() {
        let msg = error_message(
            r#"{"error":{"message":"Incorrect API key: sk-proj-abcdefghijklmnopqrstuvwx"}}"#,
        );
        assert!(!msg.contains("sk-proj"), "{msg}");
        let long = "x".repeat(5000);
        assert_eq!(error_message(&long).chars().count(), MAX_MESSAGE_CHARS);
    }
}
