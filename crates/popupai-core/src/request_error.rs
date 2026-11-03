use popupai_ai::ProviderError;
use popupai_security::{StoreError, redact};

use crate::config::{ProviderEntry, ProviderKind};
use crate::credentials::CredentialError;

/// Why a question could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Credentials(#[from] CredentialError),
    #[error("no provider is configured")]
    NoProvider,
    #[error("no model is selected")]
    NoModel,
}

impl RequestError {
    /// A short stable label that is safe to log (never message text).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NoProvider => "no_provider",
            Self::NoModel => "no_model",
            Self::Credentials(CredentialError::OriginChanged { .. }) => "origin_changed",
            Self::Credentials(CredentialError::Store(_)) => "secret_store",
            Self::Credentials(_) => "credentials",
            Self::Provider(err) => match err {
                ProviderError::Auth { .. } => "auth",
                ProviderError::MissingApiKey => "missing_key",
                ProviderError::RateLimited { .. } => "rate_limited",
                ProviderError::Unreachable => "unreachable",
                ProviderError::Network(_) => "network",
                ProviderError::Timeout => "timeout",
                ProviderError::InvalidEndpoint(_) => "invalid_endpoint",
                ProviderError::InvalidRequest(_) => "invalid_request",
                ProviderError::RedirectRefused { .. } => "redirect_refused",
                ProviderError::Api { .. } => "api",
                ProviderError::Stream(_) => "stream",
                ProviderError::Decode(_) => "decode",
            },
        }
    }

    /// Text for the popup: says what happened and what to do about it. Always
    /// passed through [`redact`], so a key echoed by a server cannot show up.
    pub fn user_message(&self, provider: Option<&ProviderEntry>) -> String {
        let name = provider.map_or("the provider", ProviderEntry::display_name);
        let text = match self {
            Self::NoProvider => {
                "No AI provider is configured. Add one to the configuration file.".to_owned()
            }
            Self::NoModel => format!(
                "No model is selected for {name}. Set `model` for it in the configuration file."
            ),
            Self::Credentials(CredentialError::OriginChanged { .. }) => format!(
                "The endpoint for {name} changed after its API key was saved, so the key was not sent. Paste the key again below to confirm the new endpoint."
            ),
            Self::Credentials(CredentialError::Store(StoreError::Unavailable(reason))) => format!(
                "The API key for {name} cannot be read because no secure secret store is available ({reason})."
            ),
            Self::Credentials(other) => {
                format!("The API key for {name} could not be used: {other}.")
            }
            Self::Provider(err) => provider_message(err, provider, name),
        };
        redact(&text)
    }
}

fn provider_message(err: &ProviderError, provider: Option<&ProviderEntry>, name: &str) -> String {
    match err {
        ProviderError::Auth { .. } if provider.is_some_and(|p| p.kind.signs_in()) => {
            format!("The {name} sign-in expired or was refused. Sign in again below.")
        }
        ProviderError::Auth { .. } => {
            format!("{name} rejected the API key. Paste a new one below.")
        }
        ProviderError::MissingApiKey => {
            format!("No API key is set for {name}. Paste one below.")
        }
        ProviderError::RateLimited {
            retry_after: Some(wait),
        } => format!(
            "{name} is limiting requests. Try again in {} s.",
            wait.as_secs().max(1)
        ),
        ProviderError::RateLimited { retry_after: None } => {
            format!("{name} is limiting requests. Try again shortly.")
        }
        ProviderError::Unreachable => match provider {
            Some(p) if p.kind == ProviderKind::Ollama => {
                format!("Could not reach Ollama at {}. Is it running?", address(p))
            }
            Some(p) => format!(
                "Could not reach {name} at {}. Check the endpoint and your connection.",
                address(p)
            ),
            None => "Could not reach the server.".to_owned(),
        },
        ProviderError::Network(detail) => {
            format!("Network error while talking to {name}: {detail}")
        }
        ProviderError::Timeout => format!("{name} did not answer in time."),
        ProviderError::InvalidEndpoint(e) => {
            format!("The endpoint configured for {name} is not valid: {e}.")
        }
        ProviderError::InvalidRequest(detail) => {
            format!("The request could not be sent: {detail}.")
        }
        ProviderError::RedirectRefused { .. } => format!(
            "The server for {name} tried to redirect the request. Redirects are refused so your key cannot be forwarded; check the endpoint URL."
        ),
        ProviderError::Api { status, message } => {
            format!("{name} returned an error (HTTP {status}): {message}")
        }
        ProviderError::Stream(detail) => format!("{name} reported an error: {detail}"),
        ProviderError::Decode(detail) => format!("Unexpected response from {name}: {detail}."),
    }
}

fn address(provider: &ProviderEntry) -> &str {
    provider
        .endpoint
        .as_deref()
        .or(provider.kind.default_endpoint())
        .unwrap_or("its configured address")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn entry(kind: ProviderKind) -> ProviderEntry {
        let mut e = ProviderEntry::new("main", kind);
        e.name = Some("My provider".into());
        e
    }

    fn say(err: impl Into<RequestError>, provider: &ProviderEntry) -> String {
        err.into().user_message(Some(provider))
    }

    #[test]
    fn local_servers_that_are_down_get_a_specific_hint() {
        let ollama = entry(ProviderKind::Ollama);
        let msg = say(ProviderError::Unreachable, &ollama);
        assert!(
            msg.contains("Ollama")
                && msg.contains("127.0.0.1:11434")
                && msg.contains("Is it running"),
            "{msg}"
        );

        let mut other = entry(ProviderKind::OpenAiCompatible);
        other.endpoint = Some("http://localhost:1234/v1".into());
        let msg = say(ProviderError::Unreachable, &other);
        assert!(
            msg.contains("My provider") && msg.contains("localhost:1234"),
            "{msg}"
        );
    }

    #[test]
    fn messages_name_the_provider_and_say_what_to_do() {
        let p = entry(ProviderKind::OpenAi);
        let auth = say(
            ProviderError::Auth {
                status: 401,
                message: "x".into(),
            },
            &p,
        );
        assert!(
            auth.contains("My provider") && auth.contains("Paste a new one"),
            "{auth}"
        );
        assert!(say(ProviderError::MissingApiKey, &p).contains("Paste one below"));
        assert!(say(ProviderError::Timeout, &p).contains("did not answer in time"));
        let limited = say(
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(7)),
            },
            &p,
        );
        assert!(limited.contains("7 s"), "{limited}");
        assert!(say(ProviderError::RateLimited { retry_after: None }, &p).contains("shortly"));
        assert!(say(ProviderError::RedirectRefused { status: 302 }, &p).contains("redirect"));
        assert!(
            say(
                ProviderError::Api {
                    status: 500,
                    message: "boom".into()
                },
                &p
            )
            .contains("HTTP 500")
        );
        assert!(
            RequestError::NoProvider
                .user_message(None)
                .contains("configuration file")
        );
        assert!(
            RequestError::NoModel
                .user_message(Some(&p))
                .contains("My provider")
        );
    }

    #[test]
    fn credential_problems_explain_the_safety_decision() {
        let p = entry(ProviderKind::OpenAi);
        let moved = say(
            CredentialError::OriginChanged {
                provider: "main".into(),
            },
            &p,
        );
        assert!(
            moved.contains("was not sent") && moved.contains("Paste the key again"),
            "{moved}"
        );
        let locked = say(
            CredentialError::Store(StoreError::Unavailable("no Secret Service".into())),
            &p,
        );
        assert!(
            locked.contains("no secure secret store") && locked.contains("no Secret Service"),
            "{locked}"
        );
    }

    #[test]
    fn user_messages_never_carry_key_like_text_from_a_server() {
        let p = entry(ProviderKind::OpenAi);
        let msg = say(
            ProviderError::Api {
                status: 400,
                message: "bad key sk-proj-abcdefghijklmnopqrstuvwx".into(),
            },
            &p,
        );
        assert!(!msg.contains("sk-proj"), "{msg}");
    }

    #[test]
    fn kinds_are_short_labels_without_message_text() {
        assert_eq!(RequestError::NoProvider.kind(), "no_provider");
        let auth = RequestError::from(ProviderError::Auth {
            status: 401,
            message: "secret detail".into(),
        });
        assert_eq!(auth.kind(), "auth");
        let moved = RequestError::from(CredentialError::OriginChanged {
            provider: "p".into(),
        });
        assert_eq!(moved.kind(), "origin_changed");
    }
}
