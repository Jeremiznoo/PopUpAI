//! Application configuration.
//!
//! The file holds settings only. **It never holds API keys**: a key found in it
//! is rejected, and keys live in the operating system secret store. Even so the
//! file is protected, because a tampered endpoint could redirect traffic: it is
//! written atomically with mode `0600`, and a file that other users can write to
//! is refused.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use popupai_security::{Endpoint, EndpointError, HttpPolicy, redact};
use serde::{Deserialize, Serialize};

use crate::session::{DismissPolicy, SessionPolicy};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SHORTCUT: &str = "CTRL+SHIFT+SPACE";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_PROVIDERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub schema_version: u32,
    /// Id of the provider used when none is chosen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    pub general: General,
    pub popup: Popup,
    pub session: SessionSettings,
    pub logging: Logging,
    pub advanced: Advanced,
    #[serde(rename = "provider")]
    pub providers: Vec<ProviderEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            default_provider: None,
            general: General::default(),
            popup: Popup::default(),
            session: SessionSettings::default(),
            logging: Logging::default(),
            advanced: Advanced::default(),
            providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    pub shortcut: String,
    pub launch_at_startup: bool,
    pub theme: Theme,
}

impl Default for General {
    fn default() -> Self {
        Self {
            shortcut: DEFAULT_SHORTCUT.to_owned(),
            launch_at_startup: false,
            theme: Theme::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Popup {
    pub corner: Corner,
    pub width: u32,
    pub initial_height: u32,
    pub max_height: u32,
}

impl Default for Popup {
    fn default() -> Self {
        Self {
            corner: Corner::default(),
            width: 480,
            initial_height: 200,
            max_height: 600,
        }
    }
}

/// What happens to an answer still being generated when the popup is dismissed
/// (see ADR 0003).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SessionSettings {
    pub cancel_on_dismiss: bool,
    /// How long a background answer stays available.
    pub background_answer_secs: u64,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            cancel_on_dismiss: false,
            background_answer_secs: 600,
        }
    }
}

impl SessionSettings {
    pub fn policy(&self) -> SessionPolicy {
        SessionPolicy {
            dismiss: if self.cancel_on_dismiss {
                DismissPolicy::CancelOnDismiss
            } else {
                DismissPolicy::FinishInBackground
            },
            detached_ttl: Duration::from_secs(self.background_answer_secs),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

/// No level ever logs prompts, answers or keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Logging {
    pub level: LogLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Advanced {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Default for providers that do not set their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub timeout_secs: u64,
    /// Tell the model which system it is talking to (distribution and version, kernel,
    /// architecture, desktop, shell, language, date) so its commands and advice fit.
    /// See `system_info` for exactly what is sent.
    pub share_system_info: bool,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            system_prompt: None,
            temperature: None,
            max_tokens: None,
            timeout_secs: 120,
            share_system_info: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "ollama")]
    Ollama,
    /// A ChatGPT subscription, signed in with a device code (no API key). Experimental: it
    /// uses an undocumented backend, see `popupai-ai`'s `oauth` module.
    #[serde(rename = "chatgpt")]
    ChatGpt,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl ProviderKind {
    /// `None` for kinds that always need an explicit endpoint.
    pub fn default_endpoint(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
            Self::Gemini => Some("https://generativelanguage.googleapis.com/v1beta"),
            Self::Ollama => Some("http://127.0.0.1:11434"),
            Self::ChatGpt => Some("https://chatgpt.com/backend-api/codex"),
            Self::OpenAiCompatible => None,
        }
    }

    /// The model used when none is configured: the lightest and cheapest the vendor
    /// offers, since the popup is for quick questions. These names come from the
    /// vendors' own model pages (consulted 2026-11-02); they are free text in the
    /// configuration and are meant to be overridden. Empty for Ollama, where the
    /// smallest installed model is picked at request time, and for compatible
    /// servers, which can serve anything.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-haiku-5-5",
            Self::OpenAi | Self::ChatGpt => "gpt-6-luna",
            Self::Gemini => "gemini-3.5-flash-lite",
            Self::Ollama | Self::OpenAiCompatible => "",
        }
    }

    /// Whether the service cannot be used without an API key.
    pub fn requires_key(self) -> bool {
        matches!(
            self,
            Self::OpenAi | Self::Anthropic | Self::Gemini | Self::ChatGpt
        )
    }

    /// The credential is obtained by signing in in a browser, not by pasting a key.
    pub fn signs_in(self) -> bool {
        matches!(self, Self::ChatGpt)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    /// Stable identifier, also used to name the stored key.
    pub id: String,
    pub kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Base URL. Defaults per kind where there is a well-known one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Free text: models are never taken from a closed list.
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Explicit opt-in to plain `http` on a network (not loopback). Traffic,
    /// including the API key, is then readable by anyone on the path.
    #[serde(default)]
    pub allow_insecure_http: bool,
}

impl ProviderEntry {
    pub fn new(id: impl Into<String>, kind: ProviderKind) -> Self {
        Self {
            id: id.into(),
            kind,
            name: None,
            endpoint: None,
            model: String::new(),
            temperature: None,
            max_tokens: None,
            timeout_secs: None,
            allow_insecure_http: false,
        }
    }

    /// The model to ask for: the configured one, else the kind's lightest default.
    /// Empty only for kinds that have no default (see [`ProviderKind::default_model`]).
    pub fn effective_model(&self) -> &str {
        let model = self.model.trim();
        if model.is_empty() {
            self.kind.default_model()
        } else {
            model
        }
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// The validated base URL, using the kind's default when none is set.
    pub fn endpoint(&self) -> Result<Endpoint, EndpointError> {
        let raw = self
            .endpoint
            .as_deref()
            .or(self.kind.default_endpoint())
            .ok_or_else(|| EndpointError::Invalid("no endpoint is configured".into()))?;
        let policy = if self.allow_insecure_http {
            HttpPolicy::AllowInsecureRemote
        } else {
            HttpPolicy::LoopbackOnly
        };
        Endpoint::parse(raw, policy)
    }

    pub fn timeout(&self, advanced: &Advanced) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(advanced.timeout_secs))
    }
}

/// One thing wrong with a configuration, phrased for a settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    pub field: String,
    pub message: String,
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The text is not valid TOML for this schema. Already redacted.
    #[error("the configuration could not be read: {0}")]
    Parse(String),
    #[error(
        "`{field}` looks like a secret. API keys must not be stored in the configuration file; paste them in the popup's key field so they go to the system secret store"
    )]
    SecretInFile { field: String },
    #[error("invalid configuration: {}", join(.0))]
    Invalid(Vec<ConfigIssue>),
    #[error("{0}")]
    Io(String),
    #[error(
        "{path} can be modified by other users. Run `chmod 600` on it (or delete it) before PopUpAI will use it"
    )]
    InsecurePermissions { path: String },
}

fn join(issues: &[ConfigIssue]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

const SECRET_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "key",
    "secret",
    "client_secret",
    "token",
    "access_token",
    "auth_token",
    "password",
    "passphrase",
    "authorization",
    "bearer",
];

fn find_secret_field(value: &toml::Value, path: &str) -> Option<String> {
    match value {
        toml::Value::Table(table) => table.iter().find_map(|(name, inner)| {
            let here = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            };
            if SECRET_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
                Some(here)
            } else {
                find_secret_field(inner, &here)
            }
        }),
        toml::Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, inner)| find_secret_field(inner, &format!("{path}[{i}]"))),
        _ => None,
    }
}

impl Config {
    /// Parse and validate. Nothing from the input text is echoed into a secret
    /// error, and parse errors pass through [`redact`].
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| ConfigError::Parse(redact(&e.to_string())))?;
        if let Some(field) = find_secret_field(&toml::Value::Table(table.clone()), "") {
            return Err(ConfigError::SecretInFile { field });
        }
        let config: Self = table
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Parse(redact(&e.to_string())))?;
        config.validate().map_err(ConfigError::Invalid)?;
        Ok(config)
    }

    /// The configuration of a first run: one entry per well-known provider, each on
    /// its lightest model, so the popup works as soon as a key is added (or, for
    /// Ollama, a model is installed). Nothing is written to disk by this.
    pub fn builtin() -> Self {
        let entry = |id: &str, kind: ProviderKind, name: &str| ProviderEntry {
            name: Some(name.to_owned()),
            ..ProviderEntry::new(id, kind)
        };
        Self {
            providers: vec![
                entry("anthropic", ProviderKind::Anthropic, "Claude"),
                entry("openai", ProviderKind::OpenAi, "OpenAI"),
                entry("gemini", ProviderKind::Gemini, "Gemini"),
                entry("chatgpt", ProviderKind::ChatGpt, "ChatGPT"),
                entry("ollama", ProviderKind::Ollama, "Ollama"),
            ],
            ..Self::default()
        }
    }

    /// Add the well-known providers that this configuration does not define, in memory. A
    /// file saved by an earlier version lacks the providers added since (ChatGPT, for one);
    /// without this they would never show up. A provider the file defines under the same id
    /// is left exactly as written.
    #[must_use]
    pub fn with_missing_builtins(mut self) -> Self {
        for builtin in Self::builtin().providers {
            if self.providers.iter().all(|p| p.id != builtin.id) {
                self.providers.push(builtin);
            }
        }
        self
    }

    /// The configured global shortcut, parsed.
    pub fn shortcut(&self) -> Result<crate::shortcut::Chord, crate::shortcut::ChordError> {
        self.general.shortcut.parse()
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderEntry> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// The provider to use when the user has not picked one.
    pub fn default_provider(&self) -> Option<&ProviderEntry> {
        match &self.default_provider {
            Some(id) => self.provider(id),
            None => self.providers.first(),
        }
    }

    /// Every problem at once, so a settings screen can show them together.
    pub fn validate(&self) -> Result<(), Vec<ConfigIssue>> {
        let mut issues = Vec::new();
        let mut bad = |field: &str, message: &str| {
            issues.push(ConfigIssue {
                field: field.to_owned(),
                message: message.to_owned(),
            });
        };

        if self.schema_version != SCHEMA_VERSION {
            bad("schema_version", "unsupported configuration version");
        }
        if let Err(err) = self.general.shortcut.parse::<crate::shortcut::Chord>() {
            bad("general.shortcut", &err.to_string());
        }
        let popup = &self.popup;
        if !(320..=1200).contains(&popup.width) {
            bad("popup.width", "must be between 320 and 1200 pixels");
        }
        if !(120..=1200).contains(&popup.initial_height) {
            bad(
                "popup.initial_height",
                "must be between 120 and 1200 pixels",
            );
        }
        if popup.max_height < popup.initial_height || popup.max_height > 2000 {
            bad(
                "popup.max_height",
                "must be at least the initial height and at most 2000 pixels",
            );
        }
        if !(1..=86_400).contains(&self.session.background_answer_secs) {
            bad(
                "session.background_answer_secs",
                "must be between 1 second and 24 hours",
            );
        }
        let advanced = &self.advanced;
        if advanced
            .system_prompt
            .as_ref()
            .is_some_and(|s| s.chars().count() > 8000)
        {
            bad("advanced.system_prompt", "must be at most 8000 characters");
        }
        if !valid_temperature(advanced.temperature) {
            bad("advanced.temperature", "must be between 0 and 2");
        }
        if advanced.max_tokens == Some(0) {
            bad("advanced.max_tokens", "must be at least 1");
        }
        if !(1..=3600).contains(&advanced.timeout_secs) {
            bad(
                "advanced.timeout_secs",
                "must be between 1 and 3600 seconds",
            );
        }

        if self.providers.len() > MAX_PROVIDERS {
            bad("provider", "too many providers");
        }
        let mut seen = std::collections::HashSet::new();
        for (i, p) in self.providers.iter().enumerate() {
            let at = |name: &str| format!("provider[{i}].{name}");
            let id_ok = (1..=48).contains(&p.id.len())
                && p.id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
                });
            if !id_ok {
                bad(
                    &at("id"),
                    "must be 1 to 48 characters from a-z, 0-9, '.', '_' and '-'",
                );
            } else if !seen.insert(p.id.as_str()) {
                bad(&at("id"), "is used by another provider");
            }
            if p.name
                .as_ref()
                .is_some_and(|n| n.chars().count() > 64 || n.chars().any(char::is_control))
            {
                bad(&at("name"), "must be at most 64 printable characters");
            }
            if let Err(e) = p.endpoint() {
                bad(&at("endpoint"), &e.to_string());
            }
            if p.model.len() > 200 || p.model.chars().any(char::is_control) {
                bad(&at("model"), "must be at most 200 printable characters");
            }
            if !valid_temperature(p.temperature) {
                bad(&at("temperature"), "must be between 0 and 2");
            }
            if p.max_tokens == Some(0) {
                bad(&at("max_tokens"), "must be at least 1");
            }
            if p.timeout_secs.is_some_and(|t| !(1..=3600).contains(&t)) {
                bad(&at("timeout_secs"), "must be between 1 and 3600 seconds");
            }
        }
        if let Some(id) = &self.default_provider
            && self.provider(id).is_none()
        {
            bad("default_provider", "does not match any configured provider");
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }

    /// Read a configuration file. A missing file means the first-run configuration
    /// ([`Config::builtin`]).
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::builtin()),
            Err(e) => return Err(io_error("read", path, &e)),
        };
        if !meta.is_file() || meta.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::Io(format!(
                "{} is not a regular configuration file of reasonable size",
                path.display()
            )));
        }
        reject_shared_write(path, &meta)?;
        let text = fs::read_to_string(path).map_err(|e| io_error("read", path, &e))?;
        Self::from_toml(&text)
    }

    /// Validate, then replace the file atomically with mode `0600` (and create
    /// missing parent directories with `0700`).
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate().map_err(ConfigError::Invalid)?;
        let text = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Io(format!("could not serialise the configuration: {e}")))?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        create_private_dir(dir).map_err(|e| io_error("create the folder for", path, &e))?;

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config.toml");
        let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let written = write_private(&tmp, text.as_bytes()).and_then(|()| fs::rename(&tmp, path));
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(io_error("write", path, &e));
        }
        if let Ok(dir_handle) = fs::File::open(dir) {
            let _ = dir_handle.sync_all(); // best effort: make the rename durable
        }
        Ok(())
    }
}

fn valid_temperature(t: Option<f32>) -> bool {
    t.is_none_or(|t| t.is_finite() && (0.0..=2.0).contains(&t))
}

fn io_error(action: &str, path: &Path, err: &std::io::Error) -> ConfigError {
    ConfigError::Io(format!("could not {action} {}: {err}", path.display()))
}

#[cfg(unix)]
fn reject_shared_write(path: &Path, meta: &fs::Metadata) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(ConfigError::InsecurePermissions {
            path: path.display().to_string(),
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_shared_write(_path: &Path, _meta: &fs::Metadata) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(unix)]
pub(crate) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
pub(crate) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "popupai-config-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample() -> Config {
        let mut openai = ProviderEntry::new("openai", ProviderKind::OpenAi);
        openai.model = "gpt-4o-mini".into();
        openai.temperature = Some(0.7);
        let mut local = ProviderEntry::new("local-llm", ProviderKind::OpenAiCompatible);
        local.name = Some("LM Studio".into());
        local.endpoint = Some("http://localhost:1234/v1".into());
        let mut ollama = ProviderEntry::new("ollama", ProviderKind::Ollama);
        ollama.model = "llama3.2:latest".into();
        Config {
            default_provider: Some("ollama".into()),
            providers: vec![openai, local, ollama],
            advanced: Advanced {
                system_prompt: Some("Answer briefly.".into()),
                ..Advanced::default()
            },
            ..Config::default()
        }
    }

    fn issues_of(config: &Config) -> Vec<String> {
        config
            .validate()
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|i| i.field)
            .collect()
    }

    #[test]
    fn defaults_are_valid_and_match_the_product_spec() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.general.shortcut, "CTRL+SHIFT+SPACE");
        assert_eq!(config.popup.corner, Corner::BottomRight);
        assert_eq!(
            (
                config.popup.width,
                config.popup.initial_height,
                config.popup.max_height
            ),
            (480, 200, 600)
        );
        assert!(!config.session.cancel_on_dismiss);
        assert!(config.providers.is_empty());
        assert!(config.default_provider().is_none());
    }

    #[test]
    fn round_trips_through_toml() {
        let config = sample();
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), config);
        assert!(text.contains("[[provider]]"), "{text}");
        assert!(text.contains("kind = \"openai-compatible\""), "{text}");
    }

    #[test]
    fn an_empty_file_gives_defaults_and_partial_files_fill_in_the_rest() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
        let config = Config::from_toml("[general]\ntheme = \"dark\"\n").unwrap();
        assert_eq!(config.general.theme, Theme::Dark);
        assert_eq!(config.general.shortcut, DEFAULT_SHORTCUT);
    }

    #[test]
    fn keys_in_the_file_are_rejected_without_echoing_them() {
        let text = "[[provider]]\nid = \"openai\"\nkind = \"openai\"\napi_key = \"zz-not-a-known-prefix-123456\"\n";
        let error = Config::from_toml(text).unwrap_err();
        assert_eq!(
            error,
            ConfigError::SecretInFile {
                field: "provider[0].api_key".into()
            }
        );
        assert!(!error.to_string().contains("zz-not"), "{error}");
        for name in ["Token", "PASSWORD", "authorization"] {
            let text = format!("[advanced]\n{name} = \"abc\"\n");
            assert!(
                matches!(
                    Config::from_toml(&text),
                    Err(ConfigError::SecretInFile { .. })
                ),
                "{name}"
            );
        }
        // Legitimate fields containing "token" are not mistaken for secrets.
        assert!(Config::from_toml("[advanced]\nmax_tokens = 100\n").is_ok());
    }

    #[test]
    fn unknown_fields_and_bad_syntax_are_errors_that_do_not_leak_key_like_text() {
        let unknown = Config::from_toml("[general]\nshortcutt = \"x\"\n").unwrap_err();
        assert!(matches!(unknown, ConfigError::Parse(_)), "{unknown}");
        let leaky =
            Config::from_toml("[general]\ntheme = sk-proj-abcdefghijklmnopqrstuvwx\n").unwrap_err();
        assert!(matches!(leaky, ConfigError::Parse(_)));
        assert!(!leaky.to_string().contains("sk-proj"), "{leaky}");
    }

    #[test]
    fn the_shortcut_is_parsed_and_a_bad_one_is_refused_with_the_reason() {
        let config = sample();
        let chord = config.shortcut().expect("the default shortcut is valid");
        assert_eq!(chord.to_string(), DEFAULT_SHORTCUT);

        let mut config = sample();
        config.general.shortcut = "SPACE".into();
        let issues = config.validate().unwrap_err();
        let issue = issues
            .iter()
            .find(|i| i.field == "general.shortcut")
            .expect("the shortcut is reported");
        assert!(issue.message.contains("no modifier"), "{}", issue.message);
    }

    #[test]
    fn validation_reports_every_problem_with_its_field() {
        let mut config = sample();
        config.schema_version = 9;
        config.general.shortcut = String::new();
        config.popup.width = 10;
        config.popup.max_height = 50;
        config.session.background_answer_secs = 0;
        config.advanced.temperature = Some(3.0);
        config.advanced.timeout_secs = 0;
        config.providers[0].id = "Bad Id".into();
        config.providers[1].id = "ollama".into();
        config.providers[2].endpoint = Some("http://example.com".into());
        config.default_provider = Some("nope".into());
        let fields = issues_of(&config);
        for expected in [
            "schema_version",
            "general.shortcut",
            "popup.width",
            "popup.max_height",
            "session.background_answer_secs",
            "advanced.temperature",
            "advanced.timeout_secs",
            "provider[0].id",
            // "ollama" is used twice; the second use is the one reported.
            "provider[2].id",
            "provider[2].endpoint",
            "default_provider",
        ] {
            assert!(
                fields.iter().any(|f| f == expected),
                "missing {expected} in {fields:?}"
            );
        }
    }

    #[test]
    fn endpoints_default_per_kind_and_follow_the_http_policy() {
        let openai = ProviderEntry::new("o", ProviderKind::OpenAi);
        assert_eq!(
            openai.endpoint().unwrap().as_str(),
            "https://api.openai.com/v1"
        );
        let ollama = ProviderEntry::new("l", ProviderKind::Ollama);
        assert_eq!(
            ollama.endpoint().unwrap().as_str(),
            "http://127.0.0.1:11434/"
        );
        let compat = ProviderEntry::new("c", ProviderKind::OpenAiCompatible);
        assert!(
            compat.endpoint().is_err(),
            "a compatible server needs an explicit endpoint"
        );

        let mut remote = ProviderEntry::new("r", ProviderKind::OpenAiCompatible);
        remote.endpoint = Some("http://192.168.1.50:8000/v1".into());
        assert!(remote.endpoint().is_err());
        remote.allow_insecure_http = true;
        assert!(remote.endpoint().is_ok());
    }

    #[test]
    fn default_provider_falls_back_to_the_first_one() {
        let mut config = sample();
        assert_eq!(config.default_provider().unwrap().id, "ollama");
        config.default_provider = None;
        assert_eq!(config.default_provider().unwrap().id, "openai");
    }

    #[test]
    fn the_first_run_configuration_is_valid_and_uses_the_lightest_models() {
        let config = Config::builtin();
        config
            .validate()
            .expect("the built-in configuration is valid");
        let models: Vec<(&str, &str)> = config
            .providers
            .iter()
            .map(|p| (p.id.as_str(), p.effective_model()))
            .collect();
        assert_eq!(
            models,
            [
                ("anthropic", "claude-haiku-5-5"),
                ("openai", "gpt-6-luna"),
                ("gemini", "gemini-3.5-flash-lite"),
                ("chatgpt", "gpt-6-luna"),
                ("ollama", ""),
            ]
        );
        // None of them carries a key or a non-default endpoint.
        assert!(config.providers.iter().all(|p| p.endpoint.is_none()));
        let reloaded = Config::from_toml(&toml::to_string_pretty(&config).unwrap()).unwrap();
        assert_eq!(reloaded, config);
    }

    #[test]
    fn providers_added_since_the_file_was_saved_are_added_but_never_override_it() {
        let mut saved = Config::builtin();
        saved.providers.retain(|p| p.id != "chatgpt");
        saved.providers[0].model = "my-model".into();
        let merged = saved.with_missing_builtins();
        let ids: Vec<&str> = merged.providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["anthropic", "openai", "gemini", "ollama", "chatgpt"]);
        assert_eq!(
            merged.providers[0].model, "my-model",
            "the file's own entry is kept"
        );
        merged.validate().unwrap();

        // An empty configuration (an empty file) gets them all.
        assert_eq!(Config::default().with_missing_builtins().providers.len(), 5);
    }

    #[test]
    fn a_configured_model_wins_over_the_default() {
        let mut entry = ProviderEntry::new("anthropic", ProviderKind::Anthropic);
        assert_eq!(entry.effective_model(), "claude-haiku-5-5");
        entry.model = "  claude-opus-5-5 ".into();
        assert_eq!(entry.effective_model(), "claude-opus-5-5");
        entry.model = "   ".into();
        assert_eq!(entry.effective_model(), "claude-haiku-5-5");

        let compat = ProviderEntry::new("c", ProviderKind::OpenAiCompatible);
        assert_eq!(
            compat.effective_model(),
            "",
            "no default for a custom server"
        );
    }

    #[test]
    fn session_settings_map_to_the_session_policy() {
        let mut settings = SessionSettings::default();
        assert_eq!(settings.policy().dismiss, DismissPolicy::FinishInBackground);
        assert_eq!(settings.policy().detached_ttl, Duration::from_secs(600));
        settings.cancel_on_dismiss = true;
        assert_eq!(settings.policy().dismiss, DismissPolicy::CancelOnDismiss);
    }

    #[test]
    fn saving_and_loading_round_trip_and_leave_no_temporary_files() {
        let tmp = TempDir::new();
        let path = tmp.0.join("config.toml");
        assert_eq!(
            Config::load(&path).unwrap(),
            Config::builtin(),
            "a missing file means the first-run configuration"
        );

        let config = sample();
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), config);

        let mut changed = config.clone();
        changed.general.theme = Theme::Light;
        changed.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().general.theme, Theme::Light);

        let leftovers: Vec<_> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["config.toml"]);
    }

    #[test]
    fn an_invalid_configuration_is_never_written() {
        let tmp = TempDir::new();
        let path = tmp.0.join("config.toml");
        let mut config = sample();
        config.general.shortcut = String::new();
        assert!(matches!(config.save(&path), Err(ConfigError::Invalid(_))));
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_shared_writable_files_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let tmp = TempDir::new();
        let path = tmp.0.join("nested").join("popupai").join("config.toml");
        sample().save(&path).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        sample().save(&path).unwrap();
        assert_eq!(mode(&path), 0o600, "rewriting keeps the file private");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o664)).unwrap();
        assert!(matches!(
            Config::load(&path),
            Err(ConfigError::InsecurePermissions { .. })
        ));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            Config::load(&path).is_ok(),
            "readable by others is acceptable, writable is not"
        );
    }

    #[test]
    fn oversized_or_non_regular_files_are_refused() {
        let tmp = TempDir::new();
        assert!(
            matches!(Config::load(&tmp.0), Err(ConfigError::Io(_))),
            "a directory is not a config file"
        );
        let big = tmp.0.join("big.toml");
        fs::write(&big, "#".repeat(MAX_CONFIG_BYTES as usize + 1)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&big, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(Config::load(&big), Err(ConfigError::Io(_))));
    }
}
