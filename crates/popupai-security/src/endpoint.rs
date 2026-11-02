use url::{Host, Url};

/// Whether plain `http` is acceptable for non-loopback hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpPolicy {
    /// `http` only for loopback (local Ollama, LM Studio, and so on).
    #[default]
    LoopbackOnly,
    /// Explicit per-provider opt-in to plain `http` on a network. The UI must
    /// warn that traffic, including the API key, is then readable on the wire.
    AllowInsecureRemote,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    #[error("not a valid URL: {0}")]
    Invalid(String),
    #[error("unsupported scheme `{0}` (only https and http are allowed)")]
    UnsupportedScheme(String),
    #[error("the URL has no host")]
    MissingHost,
    #[error("credentials must not be embedded in the URL")]
    CredentialsInUrl,
    #[error("the URL must not contain a query string or a fragment")]
    QueryOrFragment,
    #[error("plain http is only allowed for loopback addresses, not `{0}`")]
    InsecureHttp(String),
    #[error("link-local addresses are not allowed")]
    LinkLocal,
}

/// A provider base URL that passed validation.
///
/// Construction is the only way to obtain one, so code holding an `Endpoint`
/// can rely on these rules: https (or loopback http), a host, no embedded
/// credentials, no query string and no fragment. Error messages never echo the
/// input, because it may have been pasted together with a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    url: Url,
}

impl Endpoint {
    pub fn parse(input: &str, policy: HttpPolicy) -> Result<Self, EndpointError> {
        let url = Url::parse(input.trim()).map_err(|e| EndpointError::Invalid(e.to_string()))?;

        match url.scheme() {
            "https" | "http" => {}
            other => return Err(EndpointError::UnsupportedScheme(other.to_owned())),
        }
        let host = url.host().ok_or(EndpointError::MissingHost)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(EndpointError::CredentialsInUrl);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(EndpointError::QueryOrFragment);
        }
        if is_link_local(&host) {
            return Err(EndpointError::LinkLocal);
        }
        if url.scheme() == "http" && policy == HttpPolicy::LoopbackOnly && !is_loopback(&host) {
            return Err(EndpointError::InsecureHttp(host.to_string()));
        }
        Ok(Self { url })
    }

    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    pub fn is_https(&self) -> bool {
        self.url.scheme() == "https"
    }

    /// `scheme://host[:port]`, used to bind credentials to one origin.
    pub fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }

    /// Append a path to the base URL, keeping the base path (for example
    /// `https://openrouter.ai/api/v1` plus `chat/completions`).
    pub fn join(&self, path: &str) -> Url {
        let mut url = self.url.clone();
        let base = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base}/{}", path.trim_start_matches('/')));
        url
    }
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => {
            let name = name.to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        Host::Ipv4(addr) => addr.is_loopback(),
        Host::Ipv6(addr) => addr.is_loopback(),
    }
}

fn is_link_local(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(_) => false,
        Host::Ipv4(addr) => addr.is_link_local(),
        Host::Ipv6(addr) => (addr.segments()[0] & 0xffc0) == 0xfe80,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Endpoint, EndpointError> {
        Endpoint::parse(s, HttpPolicy::LoopbackOnly)
    }

    #[test]
    fn accepts_https_and_loopback_http() {
        assert!(parse("https://api.openai.com/v1").unwrap().is_https());
        assert!(!parse("http://127.0.0.1:11434").unwrap().is_https());
        assert!(parse("http://localhost:1234/v1").is_ok());
        assert!(parse("http://[::1]:8080").is_ok());
        assert!(parse("http://ollama.localhost:11434").is_ok());
    }

    #[test]
    fn rejects_plain_http_on_a_network_unless_opted_in() {
        assert_eq!(
            parse("http://example.com/v1"),
            Err(EndpointError::InsecureHttp("example.com".into()))
        );
        assert!(parse("http://192.168.1.20:11434").is_err());
        assert!(
            Endpoint::parse("http://192.168.1.20:11434", HttpPolicy::AllowInsecureRemote).is_ok()
        );
    }

    #[test]
    fn rejects_embedded_credentials_queries_and_fragments() {
        assert_eq!(
            parse("https://user:pw@example.com"),
            Err(EndpointError::CredentialsInUrl)
        );
        assert_eq!(
            parse("https://user@example.com"),
            Err(EndpointError::CredentialsInUrl)
        );
        assert_eq!(
            parse("https://example.com/v1?key=abc"),
            Err(EndpointError::QueryOrFragment)
        );
        assert_eq!(
            parse("https://example.com/v1#frag"),
            Err(EndpointError::QueryOrFragment)
        );
    }

    #[test]
    fn rejects_other_schemes_missing_hosts_and_garbage() {
        assert!(
            matches!(parse("ftp://example.com"), Err(EndpointError::UnsupportedScheme(s)) if s == "ftp")
        );
        assert!(matches!(
            parse("file:///etc/passwd"),
            Err(EndpointError::MissingHost | EndpointError::UnsupportedScheme(_))
        ));
        assert!(matches!(parse("not a url"), Err(EndpointError::Invalid(_))));
        assert!(matches!(parse(""), Err(EndpointError::Invalid(_))));
    }

    #[test]
    fn rejects_link_local_addresses_even_over_https() {
        assert_eq!(
            parse("https://169.254.169.254/latest"),
            Err(EndpointError::LinkLocal)
        );
        assert_eq!(parse("https://[fe80::1]/"), Err(EndpointError::LinkLocal));
    }

    #[test]
    fn error_messages_do_not_echo_the_input() {
        let err = parse("https://user:sk-leaked-key-123456@example.com").unwrap_err();
        assert!(!err.to_string().contains("sk-leaked"));
        let err = parse("sk-leaked-key-123456").unwrap_err();
        assert!(!err.to_string().contains("sk-leaked"));
    }

    #[test]
    fn join_keeps_the_base_path_and_handles_slashes() {
        let ep = parse("https://openrouter.ai/api/v1/").unwrap();
        assert_eq!(
            ep.join("chat/completions").as_str(),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            ep.join("/models").as_str(),
            "https://openrouter.ai/api/v1/models"
        );
        let root = parse("http://127.0.0.1:11434").unwrap();
        assert_eq!(
            root.join("api/chat").as_str(),
            "http://127.0.0.1:11434/api/chat"
        );
        assert_eq!(root.origin(), "http://127.0.0.1:11434");
    }
}
