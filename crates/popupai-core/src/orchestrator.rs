//! Request orchestration: sessions, configuration, credentials and providers
//! working together.
//!
//! The UI layer calls the synchronous methods (`open`, `submit`, `dismiss`,
//! `stop`) and reads [`Event`]s from the channel returned by [`Orchestrator::new`].
//! Network and secret-store work never runs on the caller's thread.
//!
//! Methods that start work (`submit`, `dismiss`) must be called from within a
//! Tokio runtime. Logging records only metadata (provider, model, counts, error
//! kind), never prompts, answers or keys.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use popupai_ai::chatgpt_login::{self, DeviceCode, Poll, Tokens};
use popupai_ai::{
    AgentEvent, AgentItem, AgentRequest, AnthropicProvider, ChatGptProvider, ChatMessage,
    ChatRequest, GeminiProvider, OllamaProvider, OpenAiFlavor, OpenAiProvider, Provider,
    ProviderConfig, ProviderError, Role as AiRole, ToolCall, ToolSpec,
};
use popupai_security::{Endpoint, Secret, SecretStore, StoreError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::AbortHandle;

use crate::config::{Config, ConfigIssue, ProviderEntry, ProviderKind};
use crate::credentials::{self, CredentialError};
use crate::request_error::RequestError;
use crate::session::{Effect, Message, Phase, Role, SessionId, SessionManager, SubmitError};
use crate::state::ModelRef;

/// What the UI needs to know as a request progresses.
#[derive(Clone, PartialEq, Eq)]
pub enum Event {
    Chunk {
        session: SessionId,
        text: String,
    },
    Finished {
        session: SessionId,
    },
    /// `message` is ready to show: it says what happened and what to do.
    Failed {
        session: SessionId,
        message: String,
    },
    /// The model wants to run a command that needs the user's approval: answer with
    /// [`Orchestrator::answer_permission`] using `call_id`.
    Permission {
        session: SessionId,
        call_id: String,
        command: String,
        reason: String,
    },
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunk { session, text } => {
                write!(f, "Chunk {{ session: {session}, {} bytes }}", text.len())
            }
            Self::Finished { session } => write!(f, "Finished {{ session: {session} }}"),
            Self::Failed { session, message } => {
                write!(f, "Failed {{ session: {session}, {message:?} }}")
            }
            // The command is what the user is being asked about: not shown in a debug dump.
            Self::Permission {
                session, command, ..
            } => {
                write!(
                    f,
                    "Permission {{ session: {session}, {} bytes }}",
                    command.len()
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening {
    pub session: SessionId,
    /// An answer finished or still generating in the background can be brought
    /// back with [`Orchestrator::resume_detached`].
    pub background_answer: bool,
}

/// A copy of the visible session for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    pub id: SessionId,
    pub phase: Phase,
    pub messages: Vec<Message>,
}

/// Builds a provider adapter from configuration. A trait so tests can supply
/// a scripted provider.
pub trait ProviderFactory: Send + Sync {
    fn build(
        &self,
        entry: &ProviderEntry,
        endpoint: Endpoint,
        key: Option<Secret>,
        timeout: Duration,
    ) -> Result<Arc<dyn Provider>, ProviderError>;
}

pub struct DefaultProviderFactory;

impl ProviderFactory for DefaultProviderFactory {
    fn build(
        &self,
        entry: &ProviderEntry,
        endpoint: Endpoint,
        key: Option<Secret>,
        timeout: Duration,
    ) -> Result<Arc<dyn Provider>, ProviderError> {
        let mut config = ProviderConfig::new(endpoint).with_timeout(timeout);
        if let Some(key) = key {
            config = config.with_api_key(key);
        }
        Ok(match entry.kind {
            ProviderKind::OpenAi => Arc::new(OpenAiProvider::new(config, OpenAiFlavor::OpenAi)?),
            ProviderKind::OpenAiCompatible | ProviderKind::DeepSeek => {
                Arc::new(OpenAiProvider::new(config, OpenAiFlavor::Compatible)?)
            }
            ProviderKind::Anthropic => Arc::new(AnthropicProvider::new(config)?),
            ProviderKind::Gemini => Arc::new(GeminiProvider::new(config)?),
            ProviderKind::Ollama => Arc::new(OllamaProvider::new(config)?),
            ProviderKind::ChatGpt => Arc::new(ChatGptProvider::new(config)?),
        })
    }
}

struct State {
    config: Config,
    sessions: SessionManager,
    selected: Option<String>,
    /// In-flight request tasks. Aborting one drops its stream, which closes the
    /// HTTP connection.
    tasks: HashMap<SessionId, AbortHandle>,
    /// Built providers, kept so HTTP connections are reused between questions.
    providers: HashMap<String, Arc<dyn Provider>>,
    /// Providers whose stored key was refused (by the service, or because the endpoint
    /// changed since it was saved). The popup asks for a new key until one is saved.
    rejected_keys: HashSet<String>,
    /// A model picked in the popup, which wins over the provider's configured one.
    selected_model: Option<ModelRef>,
    /// MCP servers the user switched on, and the connections already made to them.
    mcp_servers: Vec<crate::mcp::McpServer>,
    mcp_connections: HashMap<String, Arc<crate::mcp_client::McpConnection>>,
    /// Commands waiting for the user's answer, by tool call id.
    permissions: HashMap<String, tokio::sync::oneshot::Sender<bool>>,
}

struct Inner {
    state: Mutex<State>,
    store: Arc<dyn SecretStore>,
    factory: Arc<dyn ProviderFactory>,
    events: UnboundedSender<Event>,
}

impl Inner {
    /// The lock is never held across an `.await`.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event); // the UI may already be gone
    }

    fn apply(state: &mut State, effects: Vec<Effect>) {
        for Effect::CancelRequest(id) in effects {
            if let Some(task) = state.tasks.remove(&id) {
                task.abort();
            }
        }
    }

    /// Returns whether the text was accepted; `false` means the session no
    /// longer wants it (stopped or dropped) and reading should stop.
    fn deliver(&self, id: SessionId, text: String) -> bool {
        let applied = self.state().sessions.push_chunk(id, &text);
        if applied {
            self.emit(Event::Chunk { session: id, text });
        }
        applied
    }

    fn settle(&self, id: SessionId, entry: &ProviderEntry, outcome: Result<(), RequestError>) {
        let mut state = self.state();
        state.tasks.remove(&id);
        match outcome {
            Ok(()) => {
                if state.sessions.finish(id) {
                    self.emit(Event::Finished { session: id });
                }
            }
            Err(err) => {
                tracing::warn!(provider = %entry.id, error = err.kind(), "request failed");
                // A rejected or unusable key may be replaced; do not keep a
                // provider built from it.
                if matches!(err, RequestError::Credentials(_))
                    || matches!(err, RequestError::Provider(ProviderError::Auth { .. }))
                {
                    state.providers.remove(&entry.id);
                }
                if matches!(
                    err,
                    RequestError::Credentials(CredentialError::OriginChanged { .. })
                        | RequestError::Provider(ProviderError::Auth { .. })
                ) {
                    state.rejected_keys.insert(entry.id.clone());
                }
                if state.sessions.fail(id) {
                    self.emit(Event::Failed {
                        session: id,
                        message: err.user_message(Some(entry)),
                    });
                }
            }
        }
    }
}

/// Why signing in did not complete.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    #[error("the sign-in did not complete: {0}")]
    Provider(#[from] ProviderError),
    #[error("the sign-in could not be saved: {0}")]
    Credentials(#[from] CredentialError),
    #[error("the sign-in was not confirmed in time")]
    TimedOut,
}

/// A provider and whether it is ready to use, for the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionStatus {
    pub id: String,
    pub label: String,
    pub requires_key: bool,
    pub signs_in: bool,
    /// A credential is stored (always `true` for a provider that needs none).
    pub connected: bool,
}

/// One provider as the popup shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub id: String,
    /// Name and model, for display.
    pub label: String,
    /// The service cannot be used without an API key.
    pub requires_key: bool,
    /// The model the next question would use (empty: the server's smallest, for Ollama).
    pub model: String,
    /// The credential comes from signing in in a browser rather than from a pasted key.
    pub signs_in: bool,
}

/// Whether a model id from a hosted service's list is one to chat with: those lists also hold
/// embeddings, speech, image and moderation models, which cannot answer a question.
fn chat_model(id: &str) -> bool {
    const NOT_CHAT: [&str; 11] = [
        "embed",
        "whisper",
        "tts",
        "dall-e",
        "moderation",
        "image",
        "audio",
        "realtime",
        "transcribe",
        "speech",
        "imagen",
    ];
    let id = id.to_ascii_lowercase();
    !NOT_CHAT.iter().any(|word| id.contains(word))
}

/// A model that can be picked in the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub provider: String,
    pub provider_label: String,
    pub model: String,
    /// The provider needs an API key (the popup then asks for it when chosen).
    pub requires_key: bool,
}

/// The model picked in the popup for this provider, if any.
fn picked<'a>(state: &'a State, provider: &str) -> Option<&'a str> {
    state
        .selected_model
        .as_ref()
        .filter(|m| m.provider == provider)
        .map(|m| m.model.as_str())
}

fn info_of(entry: &ProviderEntry, picked: Option<&str>) -> ProviderInfo {
    let model = picked.unwrap_or_else(|| entry.effective_model());
    ProviderInfo {
        id: entry.id.clone(),
        label: if model.is_empty() {
            entry.display_name().to_owned()
        } else {
            format!("{} \u{b7} {model}", entry.display_name())
        },
        requires_key: entry.kind.requires_key(),
        model: model.to_owned(),
        signs_in: entry.kind.signs_in(),
    }
}

/// A cheap-to-clone handle. All clones share the same state.
#[derive(Clone)]
pub struct Orchestrator {
    inner: Arc<Inner>,
}

impl Orchestrator {
    pub fn new(
        config: Config,
        store: Arc<dyn SecretStore>,
        factory: Arc<dyn ProviderFactory>,
    ) -> Result<(Self, UnboundedReceiver<Event>), Vec<ConfigIssue>> {
        config.validate()?;
        let (events, receiver) = unbounded_channel();
        let state = State {
            sessions: SessionManager::new(config.session.policy()),
            config,
            selected: None,
            tasks: HashMap::new(),
            providers: HashMap::new(),
            rejected_keys: HashSet::new(),
            selected_model: None,
            permissions: HashMap::new(),
            mcp_servers: Vec::new(),
            mcp_connections: HashMap::new(),
        };
        let inner = Inner {
            state: Mutex::new(state),
            store,
            factory,
            events,
        };
        Ok((
            Self {
                inner: Arc::new(inner),
            },
            receiver,
        ))
    }

    /// Apply new settings. Requests already running finish with the settings
    /// they started with; cached providers are rebuilt on next use.
    pub fn set_config(&self, config: Config) -> Result<(), Vec<ConfigIssue>> {
        config.validate()?;
        let mut state = self.inner.state();
        state.sessions.set_policy(config.session.policy());
        if state
            .selected
            .as_deref()
            .is_some_and(|id| config.provider(id).is_none())
        {
            state.selected = None;
        }
        state.providers.clear();
        state.config = config;
        Ok(())
    }

    /// Choose the provider for the next questions. `false` if it is unknown.
    pub fn select_provider(&self, id: &str) -> bool {
        let mut state = self.inner.state();
        let known = state.config.provider(id).is_some();
        if known {
            state.selected = Some(id.to_owned());
            if state
                .selected_model
                .as_ref()
                .is_some_and(|m| m.provider != id)
            {
                state.selected_model = None;
            }
        }
        known
    }

    /// Use `model` on `provider` for the next questions. `false` if the provider is
    /// unknown or the model name is not acceptable.
    pub fn select_model(&self, provider: &str, model: &str) -> bool {
        let model = model.trim();
        let acceptable = model.len() <= 200 && !model.chars().any(char::is_control);
        let mut state = self.inner.state();
        if !acceptable || state.config.provider(provider).is_none() {
            return false;
        }
        state.selected = Some(provider.to_owned());
        state.selected_model = (!model.is_empty()).then(|| ModelRef::new(provider, model));
        true
    }

    /// The models that can be picked: each provider's configured or lightest default
    /// model, plus whatever a keyless local server says it has installed. A server that
    /// does not answer within a few seconds simply contributes nothing.
    pub async fn available_models(&self) -> Vec<ModelChoice> {
        let entries: Vec<(ProviderEntry, Duration)> = {
            let state = self.inner.state();
            let advanced = &state.config.advanced;
            state
                .config
                .providers
                .iter()
                .map(|p| (p.clone(), p.timeout(advanced)))
                .collect()
        };
        let mut choices = Vec::new();
        for (entry, timeout) in entries {
            let mut models: Vec<String> = Vec::new();
            if !entry.effective_model().is_empty() {
                models.push(entry.effective_model().to_owned());
            }
            // A local server lists what is installed; a hosted service lists its models once
            // the credential is there (an unconnected one would only answer with an error).
            let listable = !entry.kind.requires_key()
                || (!entry.kind.signs_in() && self.has_api_key(&entry.id).await.unwrap_or(false));
            if listable
                && let Ok(provider) = provider_for_entry(&self.inner, &entry, timeout).await
                && let Ok(Ok(listed)) =
                    tokio::time::timeout(Duration::from_secs(4), provider.list_models()).await
            {
                models.extend(
                    listed
                        .into_iter()
                        .map(|m| m.id)
                        .filter(|id| !entry.kind.requires_key() || chat_model(id))
                        .take(60),
                );
            }
            models.sort();
            models.dedup();
            let (label, requires_key) =
                (entry.display_name().to_owned(), entry.kind.requires_key());
            choices.extend(models.into_iter().map(|model| ModelChoice {
                provider: entry.id.clone(),
                provider_label: label.clone(),
                model,
                requires_key,
            }));
        }
        choices
    }

    /// Every configured provider, in order.
    pub fn providers(&self) -> Vec<ProviderInfo> {
        self.inner
            .state()
            .config
            .providers
            .iter()
            .map(|entry| info_of(entry, None))
            .collect()
    }

    /// The provider the next question would use.
    pub fn current_provider(&self) -> Option<ProviderInfo> {
        let state = self.inner.state();
        state
            .selected
            .as_deref()
            .and_then(|id| state.config.provider(id))
            .or_else(|| state.config.default_provider())
            .map(|entry| info_of(entry, picked(&state, &entry.id)))
    }

    /// Switch to the next provider in the list (wrapping around) and return it.
    pub fn cycle_provider(&self) -> Option<ProviderInfo> {
        let mut state = self.inner.state();
        let current = state
            .selected
            .as_deref()
            .and_then(|id| state.config.provider(id))
            .or_else(|| state.config.default_provider())
            .map(|entry| entry.id.clone());
        let providers = &state.config.providers;
        let position = current
            .as_deref()
            .and_then(|id| providers.iter().position(|p| p.id == id));
        let next = providers.get(position.map_or(0, |i| (i + 1) % providers.len()))?;
        let (id, info) = (next.id.clone(), info_of(next, None));
        state.selected = Some(id);
        state.selected_model = None;
        Some(info)
    }

    /// Without an explicit choice, use the first provider that can work right now:
    /// one whose key is stored, or one that needs none (a local server). The order
    /// of the configuration decides, so a stored key wins over a local model that
    /// comes after it.
    pub async fn auto_select(&self) {
        let candidates: Vec<(String, bool)> = {
            let state = self.inner.state();
            if state.selected.is_some() || state.config.default_provider.is_some() {
                return;
            }
            state
                .config
                .providers
                .iter()
                .map(|p| (p.id.clone(), p.kind.requires_key()))
                .collect()
        };
        for (id, requires_key) in candidates {
            if !requires_key || self.has_api_key(&id).await.unwrap_or(false) {
                self.select_provider(&id);
                return;
            }
        }
    }

    /// Every provider with whether its credential is stored. A wallet that cannot be read
    /// counts as not connected.
    pub async fn connection_status(&self) -> Vec<ConnectionStatus> {
        let entries: Vec<(String, String, bool, bool)> = {
            let state = self.inner.state();
            state
                .config
                .providers
                .iter()
                .map(|p| {
                    (
                        p.id.clone(),
                        p.display_name().to_owned(),
                        p.kind.requires_key(),
                        p.kind.signs_in(),
                    )
                })
                .collect()
        };
        let mut out = Vec::new();
        for (id, label, requires_key, signs_in) in entries {
            let rejected = self.inner.state().rejected_keys.contains(&id);
            let connected =
                !requires_key || (!rejected && self.has_api_key(&id).await.unwrap_or(false));
            out.push(ConnectionStatus {
                id,
                label,
                requires_key,
                signs_in,
                connected,
            });
        }
        out
    }

    /// The current provider needs an API key from the user: none is stored (or the
    /// store cannot be read, in which case the user needs to hear about it too), or the
    /// stored one was refused and has to be replaced.
    pub async fn current_needs_key(&self) -> bool {
        let Some(info) = self.current_provider() else {
            return false;
        };
        if self.inner.state().rejected_keys.contains(&info.id) {
            return true;
        }
        info.requires_key && !self.has_api_key(&info.id).await.unwrap_or(false)
    }

    /// A copy of the settings in use.
    pub fn config(&self) -> Config {
        self.inner.state().config.clone()
    }

    /// The MCP servers the user has switched on. Their tools are offered to the model (every
    /// call asks for approval). A server removed from the list is disconnected.
    pub fn set_mcp_servers(&self, servers: Vec<crate::mcp::McpServer>) {
        let mut state = self.inner.state();
        let keep: HashSet<&str> = servers.iter().map(|s| s.id.as_str()).collect();
        state
            .mcp_connections
            .retain(|id, _| keep.contains(id.as_str()));
        state.mcp_servers = servers;
    }

    /// The user's answer to an [`Event::Permission`].
    pub fn answer_permission(&self, call_id: &str, allow: bool) {
        if let Some(sender) = self.inner.state().permissions.remove(call_id) {
            let _ = sender.send(allow);
        }
    }

    /// The popup was opened: a fresh, empty session.
    pub fn open(&self) -> Opening {
        let mut state = self.inner.state();
        let opened = state.sessions.open(Instant::now());
        Inner::apply(&mut state, opened.effects);
        Opening {
            session: opened.id,
            background_answer: opened.detached_available,
        }
    }

    /// The popup was dismissed. Whether an answer still being generated
    /// continues is decided by the session settings (ADR 0003).
    pub fn dismiss(&self) {
        let retention = {
            let mut state = self.inner.state();
            let effects = state.sessions.dismiss(Instant::now());
            Inner::apply(&mut state, effects);
            state
                .sessions
                .detached()
                .map(|_| state.config.session.policy().detached_ttl)
        };
        if let Some(ttl) = retention {
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(ttl + Duration::from_millis(50)).await;
                this.expire();
            });
        }
    }

    /// Drop the background answer once its retention time has passed.
    pub fn expire(&self) {
        let mut state = self.inner.state();
        let effects = state.sessions.expire(Instant::now());
        Inner::apply(&mut state, effects);
    }

    /// Ask a question in the visible session. The answer arrives as events.
    /// A problem with the provider setup is also reported as an event, so the
    /// UI has a single place to show failures.
    pub fn submit(&self, prompt: &str) -> Result<SessionId, SubmitError> {
        let mut state = self.inner.state();
        let submitted = state.sessions.submit(prompt)?;
        let id = submitted.id;
        Inner::apply(&mut state, submitted.effects);

        match make_job(&state) {
            Ok(job) => {
                let task = tokio::spawn(run(self.inner.clone(), id, job));
                state.tasks.insert(id, task.abort_handle());
            }
            Err(failure) => {
                let SetupFailure { error, entry } = *failure;
                tracing::warn!(error = error.kind(), "request could not start");
                state.sessions.fail(id);
                self.inner.emit(Event::Failed {
                    session: id,
                    message: error.user_message(entry.as_ref()),
                });
            }
        }
        Ok(id)
    }

    /// The user pressed Stop: abort generation and keep what was received.
    pub fn stop(&self) {
        let mut state = self.inner.state();
        let effects = state.sessions.stop();
        Inner::apply(&mut state, effects);
    }

    /// Bring the background answer back into view.
    pub fn resume_detached(&self) -> Option<SessionId> {
        self.inner.state().sessions.resume_detached(Instant::now())
    }

    pub fn has_background_answer(&self) -> bool {
        self.inner.state().sessions.detached().is_some()
    }

    /// Name and model of the provider the next question would use, for display.
    pub fn provider_label(&self) -> Option<String> {
        self.current_provider().map(|info| info.label)
    }

    pub fn view(&self) -> Option<SessionView> {
        let state = self.inner.state();
        state.sessions.active().map(|s| SessionView {
            id: s.id(),
            phase: s.phase(),
            messages: s.messages().to_vec(),
        })
    }

    /// Save a provider's API key in the secret store, bound to the provider's
    /// current endpoint, and make the next request use it.
    pub async fn save_api_key(
        &self,
        provider_id: &str,
        key: Secret,
    ) -> Result<(), CredentialError> {
        let origin = {
            let state = self.inner.state();
            let entry = state
                .config
                .provider(provider_id)
                .ok_or_else(|| CredentialError::UnknownProvider(provider_id.to_owned()))?;
            entry
                .endpoint()
                .map_err(|e| CredentialError::InvalidEndpoint(e.to_string()))?
                .origin()
        };
        let (store, id) = (self.inner.store.clone(), provider_id.to_owned());
        blocking(move || credentials::save_key(&*store, &id, &origin, &key)).await?;
        self.inner.state().rejected_keys.remove(provider_id);
        self.invalidate_credentials(provider_id);
        Ok(())
    }

    pub async fn delete_api_key(&self, provider_id: &str) -> Result<(), CredentialError> {
        let (store, id) = (self.inner.store.clone(), provider_id.to_owned());
        blocking(move || credentials::delete_key(&*store, &id)).await?;
        self.invalidate_credentials(provider_id);
        Ok(())
    }

    pub async fn has_api_key(&self, provider_id: &str) -> Result<bool, CredentialError> {
        let (store, id) = (self.inner.store.clone(), provider_id.to_owned());
        blocking(move || credentials::has_key(&*store, &id)).await
    }

    /// Start signing in with the current provider's account (ChatGPT): returns the code
    /// to show to the user and the page where to type it.
    pub async fn begin_sign_in(&self) -> Result<DeviceCode, SignInError> {
        let client = chatgpt_login::http_client(Duration::from_secs(30))?;
        Ok(chatgpt_login::request_device_code(&client, chatgpt_login::DEFAULT_ISSUER).await?)
    }

    /// Wait until the user has confirmed in the browser, then store the tokens for
    /// `provider_id`. Gives up after 15 minutes. Dropping the future cancels it.
    pub async fn complete_sign_in(
        &self,
        provider_id: &str,
        code: &DeviceCode,
    ) -> Result<(), SignInError> {
        let origin = {
            let state = self.inner.state();
            let entry = state
                .config
                .provider(provider_id)
                .ok_or_else(|| CredentialError::UnknownProvider(provider_id.to_owned()))?;
            entry
                .endpoint()
                .map_err(|e| CredentialError::InvalidEndpoint(e.to_string()))?
                .origin()
        };
        let client = chatgpt_login::http_client(Duration::from_secs(30))?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
        let (authorization_code, code_verifier) = loop {
            match chatgpt_login::poll(&client, chatgpt_login::DEFAULT_ISSUER, code).await? {
                Poll::Confirmed {
                    authorization_code,
                    code_verifier,
                } => break (authorization_code, code_verifier),
                Poll::Pending => {
                    if tokio::time::Instant::now() + code.interval > deadline {
                        return Err(SignInError::TimedOut);
                    }
                    tokio::time::sleep(code.interval).await;
                }
            }
        };
        let tokens = chatgpt_login::exchange(
            &client,
            chatgpt_login::DEFAULT_ISSUER,
            &authorization_code,
            &code_verifier,
        )
        .await?;
        let (store, id) = (self.inner.store.clone(), provider_id.to_owned());
        let secret = tokens.to_secret();
        blocking(move || credentials::save_key(&*store, &id, &origin, &secret)).await?;
        self.inner.state().rejected_keys.remove(provider_id);
        self.invalidate_credentials(provider_id);
        Ok(())
    }

    /// Forget the cached provider so its key is read again on next use.
    pub fn invalidate_credentials(&self, provider_id: &str) {
        self.inner.state().providers.remove(provider_id);
    }
}

/// Run a blocking secret-store call off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CredentialError> + Send + 'static,
) -> Result<T, CredentialError> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|_| {
        Err(CredentialError::Store(StoreError::Backend(
            "the secret store task did not complete".into(),
        )))
    })
}

struct Job {
    entry: ProviderEntry,
    request: ChatRequest,
    timeout: Duration,
    share_system_info: bool,
    allow_commands: bool,
}

/// Why a request could not even start, with the provider it concerned.
struct SetupFailure {
    error: RequestError,
    entry: Option<ProviderEntry>,
}

/// The user's own instructions, followed by the description of their system unless that was
/// turned off. `None` when there is neither.
fn compose_instructions(own: Option<&str>, share_system_info: bool, tools: bool) -> Option<String> {
    let own = own.map(str::trim).filter(|s| !s.is_empty());
    let system = share_system_info.then(|| crate::system_info::describe(tools));
    match (own, system) {
        (Some(own), Some(system)) => Some(format!("{own}\n\n{system}")),
        (Some(own), None) => Some(own.to_owned()),
        (None, Some(system)) => Some(system),
        (None, None) => None,
    }
}

/// Capture everything a request needs while the state lock is held, so later
/// settings changes cannot affect it.
fn make_job(state: &State) -> Result<Job, Box<SetupFailure>> {
    let Some(entry) = state
        .selected
        .as_deref()
        .and_then(|id| state.config.provider(id))
        .or_else(|| state.config.default_provider())
        .cloned()
    else {
        return Err(Box::new(SetupFailure {
            error: RequestError::NoProvider,
            entry: None,
        }));
    };
    let model = picked(state, &entry.id)
        .unwrap_or_else(|| entry.effective_model())
        .to_owned();
    // Ollama may have no model set: the adapter then uses the smallest installed one.
    if model.is_empty() && entry.kind != ProviderKind::Ollama {
        return Err(Box::new(SetupFailure {
            error: RequestError::NoModel,
            entry: Some(entry),
        }));
    }
    let messages = state
        .sessions
        .active()
        .map(|session| {
            session
                .messages()
                .iter()
                .map(|m| ChatMessage {
                    role: match m.role {
                        Role::User => AiRole::User,
                        Role::Assistant => AiRole::Assistant,
                    },
                    content: m.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let advanced = &state.config.advanced;
    let request = ChatRequest {
        model,
        messages,
        system_prompt: advanced
            .system_prompt
            .clone()
            .filter(|s| !s.trim().is_empty()),
        temperature: entry.temperature.or(advanced.temperature),
        max_tokens: entry.max_tokens.or(advanced.max_tokens),
    };
    let timeout = entry.timeout(advanced);
    Ok(Job {
        entry,
        request,
        timeout,
        share_system_info: advanced.share_system_info,
        allow_commands: advanced.allow_commands,
    })
}

async fn run(inner: Arc<Inner>, id: SessionId, job: Job) {
    let outcome = answer(&inner, id, &job).await;
    inner.settle(id, &job.entry, outcome);
}

async fn answer(inner: &Arc<Inner>, id: SessionId, job: &Job) -> Result<(), RequestError> {
    let provider = provider_for(inner, job).await?;
    let tools = job.allow_commands && provider.supports_tools();
    let instructions = compose_instructions(
        job.request.system_prompt.as_deref(),
        job.share_system_info,
        tools,
    );
    tracing::info!(
        provider = %job.entry.id,
        kind = ?job.entry.kind,
        model = %job.request.model,
        messages = job.request.messages.len(),
        tools,
        "request started"
    );
    if tools {
        return agent_answer(inner, id, job, &*provider, instructions).await;
    }
    let mut request = job.request.clone();
    request.system_prompt = instructions;
    let mut stream = provider.stream_chat(request).await?;
    let mut bytes = 0usize;
    while let Some(piece) = stream.next().await {
        let text = piece?;
        bytes += text.len();
        if !inner.deliver(id, text) {
            return Ok(()); // dropping the stream closes the connection
        }
    }
    tracing::info!(provider = %job.entry.id, bytes, "request finished");
    Ok(())
}

/// Most model turns in one answer, so a model that keeps asking for commands stops.
const MAX_AGENT_STEPS: usize = 8;
/// How long the user has to answer a permission request.
const PERMISSION_WAIT: Duration = Duration::from_secs(120);

/// The conversation as a model that can run commands: it answers, may ask for a command, is
/// given the result, and carries on, until it answers without asking.
async fn agent_answer(
    inner: &Arc<Inner>,
    id: SessionId,
    job: &Job,
    provider: &dyn Provider,
    instructions: Option<String>,
) -> Result<(), RequestError> {
    let mut items: Vec<AgentItem> = job
        .request
        .messages
        .iter()
        .map(|m| match m.role {
            AiRole::User => AgentItem::User(m.content.clone()),
            AiRole::Assistant => AgentItem::Assistant(m.content.clone()),
        })
        .collect();
    let mcp = connect_mcp(inner).await;
    let mut specs = vec![ToolSpec {
        name: crate::tools::TOOL_NAME.to_owned(),
        description: "Run a shell command on the user's computer and get its output. Simple \
                      read-only commands run at once; any other command is shown to the user, who \
                      may refuse it."
            .to_owned(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command line, run with sh -c" }
            },
            "required": ["command"],
        }),
    }];
    specs.extend(mcp.iter().map(|entry| ToolSpec {
        name: entry.full_name.clone(),
        description: format!("[MCP server {}] {}", entry.server, entry.tool.description),
        parameters: entry.tool.schema.clone(),
    }));
    let mut bytes = 0usize;
    for _ in 0..MAX_AGENT_STEPS {
        let mut stream = provider
            .stream_agent(AgentRequest {
                model: job.request.model.clone(),
                instructions: instructions.clone(),
                items: items.clone(),
                tools: specs.clone(),
            })
            .await?;
        let (mut call, mut echoed) = (None, Vec::new());
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::Text(text) => {
                    bytes += text.len();
                    if !inner.deliver(id, text) {
                        return Ok(());
                    }
                }
                AgentEvent::Call(c) => {
                    call.get_or_insert(c);
                }
                AgentEvent::Raw(value) => echoed.push(value),
            }
        }
        let Some(call) = call else {
            tracing::info!(provider = %job.entry.id, bytes, "request finished");
            return Ok(());
        };
        let Some(output) = run_tool_call(inner, id, &call, &mcp).await else {
            return Ok(()); // the session was stopped meanwhile
        };
        items.extend(echoed.into_iter().map(AgentItem::Raw));
        items.push(AgentItem::Call(call.clone()));
        items.push(AgentItem::Output {
            call_id: call.call_id,
            output,
        });
    }
    let _ = inner.deliver(
        id,
        "\n\n(Stopped: too many steps in one answer.)".to_owned(),
    );
    Ok(())
}

/// A tool of an MCP server, as offered to the model.
struct McpEntry {
    server: String,
    full_name: String,
    tool: crate::mcp_client::McpTool,
    connection: Arc<crate::mcp_client::McpConnection>,
}

const MAX_MCP_TOOLS: usize = 40;

/// The name the model sees: `mcp__server__tool`, limited to what tool names allow.
fn mcp_tool_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let mut name = format!("mcp__{}__{}", clean(server), clean(tool));
    name.truncate(64);
    name
}

/// Connect to the servers the user switched on, starting the ones not running yet. A server that
/// cannot be started is skipped (and noted in the log): it must not stop the answer.
async fn connect_mcp(inner: &Arc<Inner>) -> Vec<McpEntry> {
    let servers = inner.state().mcp_servers.clone();
    let mut entries = Vec::new();
    for server in servers {
        let cached = inner.state().mcp_connections.get(&server.id).cloned();
        let connection = match cached {
            Some(connection) => connection,
            None => match crate::mcp_client::McpConnection::start(&server).await {
                Ok(connection) => {
                    let connection = Arc::new(connection);
                    inner
                        .state()
                        .mcp_connections
                        .insert(server.id.clone(), connection.clone());
                    connection
                }
                Err(_) => {
                    tracing::warn!(server = %server.id, "an MCP server could not be started");
                    continue;
                }
            },
        };
        for tool in &connection.tools {
            if entries.len() >= MAX_MCP_TOOLS {
                break;
            }
            entries.push(McpEntry {
                server: server.name.clone(),
                full_name: mcp_tool_name(&server.name, &tool.name),
                tool: tool.clone(),
                connection: connection.clone(),
            });
        }
    }
    entries
}

/// Put a command to the user and wait for the answer. `None` if the answer was stopped.
async fn ask_user(
    inner: &Arc<Inner>,
    id: SessionId,
    call_id: &str,
    command: String,
    reason: String,
) -> Option<bool> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    inner.state().permissions.insert(call_id.to_owned(), tx);
    inner.emit(Event::Permission {
        session: id,
        call_id: call_id.to_owned(),
        command,
        reason,
    });
    let allowed = tokio::time::timeout(PERMISSION_WAIT, rx)
        .await
        .is_ok_and(|answer| answer.unwrap_or(false));
    inner.state().permissions.remove(call_id);
    if !allowed && !inner.deliver(id, "*(not run)*\n\n".to_owned()) {
        return None;
    }
    Some(allowed)
}

const DECLINED: &str = "The user declined to run this. Do not retry it; answer without it or suggest what they could do themselves.";

/// Run what the model asked for, after the user's approval when it is not a plain read.
/// Returns the text to give back to the model, or `None` if the answer was stopped.
async fn run_tool_call(
    inner: &Arc<Inner>,
    id: SessionId,
    call: &ToolCall,
    mcp: &[McpEntry],
) -> Option<String> {
    let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments).ok();
    if call.name == crate::tools::TOOL_NAME {
        let command = arguments
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let Some(command) = command else {
            return Some("error: the `command` argument is missing".to_owned());
        };
        // The command is shown in the answer, so the user always sees what ran.
        if !inner.deliver(id, format!("\n\n`$ {}`\n\n", command.replace('`', "'"))) {
            return None;
        }
        if let crate::tools::Decision::Ask(reason) = crate::tools::classify(&command)
            && !ask_user(inner, id, &call.call_id, command.clone(), reason).await?
        {
            tracing::info!("a command was not approved");
            return Some(DECLINED.to_owned());
        }
        tracing::info!("a command was run");
        return Some(crate::tools::run(&command).await);
    }
    let Some(entry) = mcp.iter().find(|e| e.full_name == call.name) else {
        return Some("error: unknown tool".to_owned());
    };
    let arguments = arguments
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let mut shown = format!("{}: {}({})", entry.server, entry.tool.name, arguments);
    shown.truncate(shown.floor_char_boundary(400));
    if !inner.deliver(id, format!("\n\n`{}`\n\n", shown.replace('`', "'"))) {
        return None;
    }
    // Every MCP call asks first: the server is another program, and the arguments are the model's.
    let reason = format!("calls a tool of the MCP server \"{}\"", entry.server);
    if !ask_user(inner, id, &call.call_id, shown, reason).await? {
        tracing::info!("an MCP call was not approved");
        return Some(DECLINED.to_owned());
    }
    tracing::info!("an MCP tool was called");
    Some(entry.connection.call(&entry.tool.name, arguments).await)
}

/// The cached provider, or a new one built with the key from the secret store.
/// The key is only released if it was saved for this provider's endpoint.
async fn provider_for(inner: &Arc<Inner>, job: &Job) -> Result<Arc<dyn Provider>, RequestError> {
    provider_for_entry(inner, &job.entry, job.timeout).await
}

async fn provider_for_entry(
    inner: &Arc<Inner>,
    entry: &ProviderEntry,
    timeout: Duration,
) -> Result<Arc<dyn Provider>, RequestError> {
    let signs_in = entry.kind.signs_in();
    // A sign-in expires within the hour and renews itself, so its provider is rebuilt
    // for each request instead of being cached.
    let cached = inner.state().providers.get(&entry.id).cloned();
    if !signs_in && let Some(provider) = cached {
        return Ok(provider);
    }
    let endpoint = entry.endpoint().map_err(ProviderError::from)?;
    let origin = endpoint.origin();
    let (store, id) = (inner.store.clone(), entry.id.clone());
    let key = match blocking(move || credentials::load_key(&*store, &id, &origin)).await {
        Ok(key) => key,
        // A local server such as Ollama needs no key, so an unavailable secret
        // store must not stop it. Providers that do need a key still fail below.
        Err(CredentialError::Store(StoreError::Unavailable(_))) if !entry.kind.requires_key() => {
            tracing::warn!(
                provider = %entry.id,
                "secret store unavailable; continuing without an API key"
            );
            None
        }
        Err(err) => return Err(err.into()),
    };
    let key = if signs_in {
        match key {
            Some(stored) => Some(renewed_sign_in(inner, entry, &origin_of(entry)?, stored).await?),
            None => None,
        }
    } else {
        key
    };
    let provider = inner.factory.build(entry, endpoint, key, timeout)?;
    if !signs_in {
        inner
            .state()
            .providers
            .insert(entry.id.clone(), provider.clone());
    }
    Ok(provider)
}

fn origin_of(entry: &ProviderEntry) -> Result<String, RequestError> {
    Ok(entry.endpoint().map_err(ProviderError::from)?.origin())
}

/// A sign-in whose access token is about to expire is renewed first, and the renewed
/// tokens are stored (the server may rotate the refresh token, so keeping the old one
/// would lock the user out). A refusal means the user has to sign in again.
async fn renewed_sign_in(
    inner: &Arc<Inner>,
    entry: &ProviderEntry,
    origin: &str,
    stored: Secret,
) -> Result<Secret, RequestError> {
    let tokens = Tokens::from_secret(&stored)?;
    if !tokens.expires_within(Duration::from_secs(120)) {
        return Ok(stored);
    }
    let client = chatgpt_login::http_client(Duration::from_secs(30))?;
    let renewed = chatgpt_login::refresh(&client, chatgpt_login::DEFAULT_ISSUER, &tokens).await?;
    let secret = renewed.to_secret();
    let (store, id, origin) = (inner.store.clone(), entry.id.clone(), origin.to_owned());
    let to_store = secret.clone();
    blocking(move || credentials::save_key(&*store, &id, &origin, &to_store)).await?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use futures_util::future::BoxFuture;
    use futures_util::stream;
    use popupai_ai::{ChatStream, ModelInfo};
    use popupai_security::{MemoryStore, UnavailableStore};
    use tokio::time::timeout;

    use super::*;
    use crate::config::Advanced;

    type Feed = UnboundedReceiver<Result<String, ProviderError>>;
    type FeedSender = UnboundedSender<Result<String, ProviderError>>;

    /// A provider whose answers are pushed by the test, one feed per request.
    #[derive(Default)]
    struct Fake {
        requests: Mutex<Vec<ChatRequest>>,
        feeds: Mutex<VecDeque<Feed>>,
    }

    impl Provider for Fake {
        fn kind(&self) -> &'static str {
            "fake"
        }

        fn stream_chat(
            &self,
            request: ChatRequest,
        ) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
            self.requests.lock().unwrap().push(request);
            let feed = self.feeds.lock().unwrap().pop_front();
            Box::pin(async move {
                let stream = stream::unfold(feed, |feed| async move {
                    let mut rx = feed?;
                    let item = rx.recv().await?;
                    Some((item, Some(rx)))
                });
                Ok(Box::pin(stream) as ChatStream)
            })
        }

        fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    struct Built {
        provider: String,
        key: Option<String>,
    }

    struct FakeFactory {
        fake: Arc<Fake>,
        builds: Arc<Mutex<Vec<Built>>>,
    }

    impl ProviderFactory for FakeFactory {
        fn build(
            &self,
            entry: &ProviderEntry,
            _endpoint: Endpoint,
            key: Option<Secret>,
            _timeout: Duration,
        ) -> Result<Arc<dyn Provider>, ProviderError> {
            self.builds.lock().unwrap().push(Built {
                provider: entry.id.clone(),
                key: key.map(|k| k.expose().to_owned()),
            });
            Ok(self.fake.clone())
        }
    }

    struct Harness {
        orch: Orchestrator,
        events: UnboundedReceiver<Event>,
        fake: Arc<Fake>,
        builds: Arc<Mutex<Vec<Built>>>,
        store: Arc<MemoryStore>,
    }

    impl Harness {
        /// Script the next request: the test sends its answer through this.
        fn feed(&self) -> FeedSender {
            let (tx, rx) = unbounded_channel();
            self.fake.feeds.lock().unwrap().push_back(rx);
            tx
        }

        fn requests(&self) -> Vec<ChatRequest> {
            self.fake.requests.lock().unwrap().clone()
        }

        fn build_count(&self) -> usize {
            self.builds.lock().unwrap().len()
        }
    }

    fn config() -> Config {
        let mut provider = ProviderEntry::new("fake", ProviderKind::OpenAiCompatible);
        provider.endpoint = Some("http://127.0.0.1:9/v1".into());
        provider.model = "test-model".into();
        provider.temperature = Some(0.4);
        Config {
            providers: vec![provider],
            advanced: Advanced {
                system_prompt: Some("Be brief.".into()),
                // The tests of the exact prompt do not want the system description added.
                share_system_info: false,
                ..Advanced::default()
            },
            ..Config::default()
        }
    }

    fn harness(config: Config) -> Harness {
        let fake = Arc::new(Fake::default());
        let builds = Arc::new(Mutex::new(Vec::new()));
        let store = Arc::new(MemoryStore::default());
        let factory = Arc::new(FakeFactory {
            fake: fake.clone(),
            builds: builds.clone(),
        });
        let (orch, events) = Orchestrator::new(config, store.clone(), factory).unwrap();
        Harness {
            orch,
            events,
            fake,
            builds,
            store,
        }
    }

    async fn next(events: &mut UnboundedReceiver<Event>) -> Event {
        timeout(Duration::from_secs(3), events.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("the event channel closed")
    }

    async fn no_event(events: &mut UnboundedReceiver<Event>) {
        let got = timeout(Duration::from_millis(150), events.recv()).await;
        assert!(got.is_err(), "unexpected event: {got:?}");
    }

    /// Resolves once the request's stream has been dropped.
    async fn dropped(feed: &FeedSender) {
        timeout(Duration::from_secs(3), feed.closed())
            .await
            .expect("the provider stream should have been dropped");
    }

    fn chunk(session: SessionId, text: &str) -> Event {
        Event::Chunk {
            session,
            text: text.into(),
        }
    }

    fn texts(view: &SessionView) -> Vec<&str> {
        view.messages.iter().map(|m| m.text.as_str()).collect()
    }

    #[tokio::test]
    async fn streams_chunks_then_finishes_and_keeps_the_conversation() {
        let mut h = harness(config());
        let feed = h.feed();
        let opening = h.orch.open();
        let id = h.orch.submit("What is DNS?").unwrap();
        assert_eq!(id, opening.session);

        feed.send(Ok("A naming ".into())).unwrap();
        feed.send(Ok("system.".into())).unwrap();
        drop(feed);

        assert_eq!(next(&mut h.events).await, chunk(id, "A naming "));
        assert_eq!(next(&mut h.events).await, chunk(id, "system."));
        assert_eq!(next(&mut h.events).await, Event::Finished { session: id });

        let view = h.orch.view().unwrap();
        assert_eq!(view.phase, Phase::Complete);
        assert_eq!(texts(&view), ["What is DNS?", "A naming system."]);

        let request = &h.requests()[0];
        assert_eq!(request.model, "test-model");
        assert_eq!(request.system_prompt.as_deref(), Some("Be brief."));
        assert_eq!(request.temperature, Some(0.4));
        assert_eq!(request.messages.len(), 1);
    }

    #[tokio::test]
    async fn every_opening_is_a_fresh_session() {
        let mut h = harness(config());
        let feed = h.feed();
        h.orch.open();
        let first = h.orch.submit("What is DNS?").unwrap();
        feed.send(Ok("answer".into())).unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        h.orch.dismiss();

        let second = h.orch.open();
        assert_ne!(second.session, first);
        assert!(!second.background_answer);
        assert!(h.orch.view().unwrap().messages.is_empty());

        // The next question must not inherit the previous conversation.
        let feed = h.feed();
        h.orch.submit("Explain this Python error.").unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        let sent = &h.requests()[1];
        assert_eq!(sent.messages.len(), 1);
        assert_eq!(sent.messages[0].content, "Explain this Python error.");
    }

    #[tokio::test]
    async fn follow_up_questions_share_the_context_of_one_opening() {
        let mut h = harness(config());
        let first = h.feed();
        h.orch.open();
        h.orch.submit("What is DNS?").unwrap();
        first.send(Ok("A naming system.".into())).unwrap();
        drop(first);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}

        let second = h.feed();
        h.orch.submit("And DHCP?").unwrap();
        drop(second);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}

        let roles: Vec<_> = h.requests()[1].messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [AiRole::User, AiRole::Assistant, AiRole::User]);
    }

    #[tokio::test]
    async fn dismissing_while_streaming_lets_the_answer_finish_in_the_background() {
        let mut h = harness(config());
        let feed = h.feed();
        h.orch.open();
        let id = h.orch.submit("long question").unwrap();
        feed.send(Ok("a".into())).unwrap();
        assert_eq!(next(&mut h.events).await, chunk(id, "a"));

        h.orch.dismiss();
        assert!(h.orch.view().is_none());
        assert!(h.orch.has_background_answer());

        feed.send(Ok("b".into())).unwrap();
        drop(feed);
        assert_eq!(next(&mut h.events).await, chunk(id, "b"));
        assert_eq!(next(&mut h.events).await, Event::Finished { session: id });

        let reopened = h.orch.open();
        assert!(reopened.background_answer);
        assert!(
            h.orch.view().unwrap().messages.is_empty(),
            "a new opening starts empty"
        );
        assert_eq!(h.orch.resume_detached(), Some(id));
        assert_eq!(texts(&h.orch.view().unwrap()), ["long question", "ab"]);
    }

    #[tokio::test]
    async fn a_new_question_cancels_the_background_request() {
        let mut h = harness(config());
        let old = h.feed();
        h.orch.open();
        let first = h.orch.submit("first").unwrap();
        old.send(Ok("partial".into())).unwrap();
        // Wait until the request is really running before dismissing it.
        assert_eq!(next(&mut h.events).await, chunk(first, "partial"));
        h.orch.dismiss();

        let new = h.feed();
        h.orch.open();
        h.orch.submit("second").unwrap();

        dropped(&old).await;
        assert!(!h.orch.has_background_answer());
        drop(new);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
    }

    #[tokio::test]
    async fn stop_aborts_the_request_keeps_the_partial_answer_and_sends_nothing_more() {
        let mut h = harness(config());
        let feed = h.feed();
        h.orch.open();
        let id = h.orch.submit("question").unwrap();
        feed.send(Ok("part".into())).unwrap();
        assert_eq!(next(&mut h.events).await, chunk(id, "part"));

        h.orch.stop();

        dropped(&feed).await;
        let view = h.orch.view().unwrap();
        assert_eq!(view.phase, Phase::Cancelled);
        assert_eq!(texts(&view), ["question", "part"]);
        no_event(&mut h.events).await;
    }

    #[tokio::test]
    async fn cancel_on_dismiss_aborts_the_request() {
        let mut settings = config();
        settings.session.cancel_on_dismiss = true;
        let mut h = harness(settings);
        let feed = h.feed();
        h.orch.open();
        let id = h.orch.submit("question").unwrap();
        feed.send(Ok("partial".into())).unwrap();
        // Wait until the request is really running before dismissing it.
        assert_eq!(next(&mut h.events).await, chunk(id, "partial"));

        h.orch.dismiss();

        dropped(&feed).await;
        assert!(!h.orch.has_background_answer());
        no_event(&mut h.events).await;
    }

    #[tokio::test]
    async fn a_background_answer_expires_and_its_request_is_cancelled() {
        let mut settings = config();
        settings.session.background_answer_secs = 1;
        let h = harness(settings);
        let feed = h.feed();
        h.orch.open();
        h.orch.submit("question").unwrap();
        h.orch.dismiss();
        assert!(h.orch.has_background_answer());

        dropped(&feed).await;
        assert!(!h.orch.has_background_answer());
    }

    #[tokio::test]
    async fn provider_errors_become_a_readable_failure_and_the_session_stays_usable() {
        let mut h = harness(config());
        let feed = h.feed();
        h.orch.open();
        let id = h.orch.submit("question").unwrap();
        feed.send(Err(ProviderError::Auth {
            status: 401,
            message: "bad key sk-proj-abcdefghijklmnopqrstuvwx".into(),
        }))
        .unwrap();

        let Event::Failed { session, message } = next(&mut h.events).await else {
            panic!("expected a failure");
        };
        assert_eq!(session, id);
        assert!(message.contains("rejected the API key"), "{message}");
        assert!(!message.contains("sk-proj"), "{message}");
        assert_eq!(h.orch.view().unwrap().phase, Phase::Failed);

        let retry = h.feed();
        h.orch.submit("again").unwrap();
        drop(retry);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        assert_eq!(
            h.build_count(),
            2,
            "a rejected key must not keep a cached provider"
        );
    }

    #[tokio::test]
    async fn setup_problems_fail_cleanly_instead_of_hanging() {
        let mut none = harness(Config::default());
        none.orch.open();
        let id = none.orch.submit("question").unwrap();
        let Event::Failed { session, message } = next(&mut none.events).await else {
            panic!("expected a failure");
        };
        assert_eq!(session, id);
        assert!(message.contains("No AI provider"), "{message}");
        assert_eq!(none.orch.view().unwrap().phase, Phase::Failed);

        let mut settings = config();
        settings.providers[0].model = "  ".into();
        let mut blank = harness(settings);
        blank.orch.open();
        blank.orch.submit("question").unwrap();
        let Event::Failed { message, .. } = next(&mut blank.events).await else {
            panic!("expected a failure");
        };
        assert!(message.contains("No model is selected"), "{message}");
        assert!(
            blank.requests().is_empty(),
            "nothing is sent without a model"
        );
    }

    #[tokio::test]
    async fn keys_come_from_the_secret_store_and_are_bound_to_the_endpoint() {
        let mut h = harness(config());
        h.orch
            .save_api_key("fake", Secret::new("  key-one\n"))
            .await
            .unwrap();
        assert!(h.orch.has_api_key("fake").await.unwrap());

        let feed = h.feed();
        h.orch.open();
        h.orch.submit("question").unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        {
            let builds = h.builds.lock().unwrap();
            assert_eq!(builds.len(), 1);
            assert_eq!(
                (builds[0].provider.as_str(), builds[0].key.as_deref()),
                ("fake", Some("key-one"))
            );
        }

        // Someone edits the endpoint in the config file: the key must not follow.
        let mut moved = config();
        moved.providers[0].endpoint = Some("http://127.0.0.1:10/v1".into());
        h.orch.set_config(moved).unwrap();
        h.orch.open();
        h.orch.submit("question").unwrap();
        let Event::Failed { message, .. } = next(&mut h.events).await else {
            panic!("expected a failure");
        };
        assert!(message.contains("was not sent"), "{message}");
        assert_eq!(
            h.build_count(),
            1,
            "no provider may be built with a key for another origin"
        );

        // Saving the key again is the user's confirmation of the new endpoint.
        h.orch
            .save_api_key("fake", Secret::new("key-two"))
            .await
            .unwrap();
        let feed = h.feed();
        h.orch.open();
        h.orch.submit("question").unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        assert_eq!(
            h.builds.lock().unwrap().last().unwrap().key.as_deref(),
            Some("key-two")
        );

        h.orch.delete_api_key("fake").await.unwrap();
        assert!(!h.orch.has_api_key("fake").await.unwrap());
        assert!(
            h.store
                .get(&credentials::origin_account("fake"))
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn providers_are_reused_so_connections_stay_warm() {
        let mut h = harness(config());
        h.orch.open();
        for _ in 0..3 {
            let feed = h.feed();
            h.orch.submit("question").unwrap();
            drop(feed);
            while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}
        }
        assert_eq!(h.build_count(), 1);
    }

    #[tokio::test]
    async fn new_settings_apply_to_the_next_question() {
        let mut h = harness(config());
        let feed = h.feed();
        h.orch.open();
        h.orch.submit("one").unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}

        let mut changed = config();
        changed.providers[0].model = "other-model".into();
        changed.advanced.system_prompt = None;
        h.orch.set_config(changed).unwrap();
        let feed = h.feed();
        h.orch.submit("two").unwrap();
        drop(feed);
        while !matches!(next(&mut h.events).await, Event::Finished { .. }) {}

        let requests = h.requests();
        assert_eq!(requests[1].model, "other-model");
        assert_eq!(requests[1].system_prompt, None);
        assert_eq!(h.build_count(), 2, "changed settings rebuild the provider");

        let mut invalid = config();
        invalid.general.shortcut = String::new();
        assert!(h.orch.set_config(invalid).is_err());
        assert!(h.orch.select_provider("fake"));
        assert!(!h.orch.select_provider("missing"));
    }

    /// An orchestrator whose secret store cannot be opened.
    struct Offline {
        orch: Orchestrator,
        events: UnboundedReceiver<Event>,
        fake: Arc<Fake>,
        builds: Arc<Mutex<Vec<Built>>>,
    }

    fn with_unavailable_store(config: Config) -> Offline {
        let fake = Arc::new(Fake::default());
        let builds = Arc::new(Mutex::new(Vec::new()));
        let factory = Arc::new(FakeFactory {
            fake: fake.clone(),
            builds: builds.clone(),
        });
        let store = Arc::new(UnavailableStore::new("no Secret Service"));
        let (orch, events) = Orchestrator::new(config, store, factory).unwrap();
        Offline {
            orch,
            events,
            fake,
            builds,
        }
    }

    #[tokio::test]
    async fn a_keyless_local_provider_works_even_when_the_secret_store_is_unavailable() {
        let Offline {
            orch,
            mut events,
            fake,
            builds,
        } = with_unavailable_store(config());
        let (tx, rx) = unbounded_channel();
        fake.feeds.lock().unwrap().push_back(rx);

        orch.open();
        orch.submit("hi").unwrap();
        tx.send(Ok("ok".into())).unwrap();
        drop(tx);

        while !matches!(next(&mut events).await, Event::Finished { .. }) {}
        let builds = builds.lock().unwrap();
        assert_eq!(builds.len(), 1);
        assert_eq!(
            builds[0].key, None,
            "no key is invented when the store cannot be read"
        );
    }

    #[tokio::test]
    async fn a_provider_that_needs_a_key_explains_that_the_secret_store_is_unavailable() {
        let mut settings = config();
        settings.providers[0].kind = ProviderKind::OpenAi;
        settings.providers[0].endpoint = None;
        let Offline {
            orch,
            mut events,
            builds,
            ..
        } = with_unavailable_store(settings);

        orch.open();
        orch.submit("hi").unwrap();

        let Event::Failed { message, .. } = next(&mut events).await else {
            panic!("expected a failure");
        };
        assert!(message.contains("no secure secret store"), "{message}");
        assert!(
            builds.lock().unwrap().is_empty(),
            "nothing is built without the key"
        );
    }

    #[tokio::test]
    async fn the_provider_label_names_the_provider_and_its_model() {
        let h = harness(config());
        assert_eq!(
            h.orch.provider_label().as_deref(),
            Some("fake \u{b7} test-model")
        );

        let mut blank = config();
        blank.providers[0].model = String::new();
        blank.providers[0].name = Some("LM Studio".into());
        assert_eq!(
            harness(blank).orch.provider_label().as_deref(),
            Some("LM Studio")
        );

        assert_eq!(harness(Config::default()).orch.provider_label(), None);
    }

    fn built_in_config() -> Config {
        Config::builtin()
    }

    #[tokio::test]
    async fn the_provider_list_shows_labels_and_which_ones_need_a_key() {
        let h = harness(built_in_config());
        let list: Vec<(String, String, bool)> = h
            .orch
            .providers()
            .into_iter()
            .map(|p| (p.id, p.label, p.requires_key))
            .collect();
        assert_eq!(
            list,
            [
                (
                    "anthropic".into(),
                    "Claude \u{b7} claude-haiku-5-5".into(),
                    true
                ),
                ("openai".into(), "OpenAI \u{b7} gpt-6-luna".into(), true),
                (
                    "gemini".into(),
                    "Gemini \u{b7} gemini-3.5-flash-lite".into(),
                    true
                ),
                (
                    "deepseek".into(),
                    "DeepSeek \u{b7} deepseek-flash".into(),
                    true
                ),
                ("chatgpt".into(), "ChatGPT \u{b7} gpt-6-luna".into(), true),
                ("ollama".into(), "Ollama".into(), false),
            ]
        );
    }

    #[tokio::test]
    async fn cycling_walks_the_list_and_wraps_around() {
        let h = harness(built_in_config());
        assert_eq!(h.orch.current_provider().unwrap().id, "anthropic");
        let seen: Vec<String> = (0..5)
            .map(|_| h.orch.cycle_provider().unwrap().id)
            .collect();
        assert_eq!(seen, ["openai", "gemini", "deepseek", "chatgpt", "ollama"]);
        assert_eq!(h.orch.current_provider().unwrap().id, "ollama");
        assert!(harness(Config::default()).orch.cycle_provider().is_none());
    }

    #[tokio::test]
    async fn auto_select_prefers_a_stored_key_then_falls_back_to_the_local_server() {
        let h = harness(built_in_config());
        h.orch.auto_select().await;
        assert_eq!(
            h.orch.current_provider().unwrap().id,
            "ollama",
            "with no key stored, the keyless local provider is the one that can work"
        );

        let h = harness(built_in_config());
        h.orch
            .save_api_key("gemini", Secret::new("g-key"))
            .await
            .unwrap();
        h.orch.auto_select().await;
        assert_eq!(h.orch.current_provider().unwrap().id, "gemini");
    }

    #[tokio::test]
    async fn auto_select_never_overrides_an_explicit_choice() {
        let mut config = built_in_config();
        config.default_provider = Some("openai".into());
        let h = harness(config);
        h.orch.auto_select().await;
        assert_eq!(h.orch.current_provider().unwrap().id, "openai");

        let h = harness(built_in_config());
        assert!(h.orch.select_provider("gemini"));
        h.orch.auto_select().await;
        assert_eq!(h.orch.current_provider().unwrap().id, "gemini");
    }

    #[tokio::test]
    async fn a_missing_key_is_detected_and_an_unreadable_store_counts_as_missing() {
        let h = harness(built_in_config());
        assert!(h.orch.current_needs_key().await, "no key for Claude yet");
        h.orch
            .save_api_key("anthropic", Secret::new("a-key"))
            .await
            .unwrap();
        assert!(!h.orch.current_needs_key().await);

        h.orch.select_provider("ollama");
        assert!(
            !h.orch.current_needs_key().await,
            "a local server needs none"
        );

        let (orch, _events) = Orchestrator::new(
            built_in_config(),
            Arc::new(UnavailableStore::new("test")),
            Arc::new(DefaultProviderFactory),
        )
        .unwrap();
        assert!(orch.current_needs_key().await);
    }

    #[tokio::test]
    async fn an_empty_model_is_sent_as_the_providers_lightest_default() {
        let mut config = built_in_config();
        config.default_provider = Some("anthropic".into());
        let h = harness(config);
        h.orch
            .save_api_key("anthropic", Secret::new("a-key"))
            .await
            .unwrap();
        h.orch.open();
        let session = h.orch.submit("hi").unwrap();
        let _ = session;
        let tx = h.feed();
        drop(tx);
        // The request is built from the effective model.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let requests = h.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "claude-haiku-5-5");
    }

    #[tokio::test]
    async fn a_refused_key_asks_for_a_new_one_until_one_is_saved() {
        let mut config = built_in_config();
        config.default_provider = Some("anthropic".into());
        let mut h = harness(config);
        h.orch
            .save_api_key("anthropic", Secret::new("wrong-key"))
            .await
            .unwrap();
        assert!(
            !h.orch.current_needs_key().await,
            "a stored key is not missing"
        );

        let feed = h.feed();
        h.orch.open();
        h.orch.submit("question").unwrap();
        feed.send(Err(ProviderError::Auth {
            status: 401,
            message: "invalid x-api-key".into(),
        }))
        .unwrap();
        let Event::Failed { message, .. } = next(&mut h.events).await else {
            panic!("expected a failure");
        };
        assert!(message.contains("Paste a new one"), "{message}");
        assert!(
            h.orch.current_needs_key().await,
            "after a refusal the popup must ask for a new key"
        );

        h.orch
            .save_api_key("anthropic", Secret::new("right-key"))
            .await
            .unwrap();
        assert!(!h.orch.current_needs_key().await);
    }

    #[tokio::test]
    async fn a_picked_model_is_used_and_shown_until_another_provider_is_chosen() {
        let mut config = built_in_config();
        config.default_provider = Some("anthropic".into());
        let h = harness(config);
        h.orch
            .save_api_key("anthropic", Secret::new("a-key"))
            .await
            .unwrap();

        assert!(h.orch.select_model("anthropic", "claude-opus-5-5"));
        assert_eq!(
            h.orch.provider_label().as_deref(),
            Some("Claude \u{b7} claude-opus-5-5")
        );
        h.orch.open();
        let feed = h.feed();
        h.orch.submit("hi").unwrap();
        drop(feed);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(h.requests()[0].model, "claude-opus-5-5");

        // Another provider goes back to its own default.
        assert!(h.orch.select_provider("openai"));
        assert_eq!(
            h.orch.provider_label().as_deref(),
            Some("OpenAI \u{b7} gpt-6-luna")
        );
    }

    #[tokio::test]
    async fn unacceptable_model_choices_are_refused() {
        let h = harness(built_in_config());
        assert!(!h.orch.select_model("nope", "x"));
        assert!(!h.orch.select_model("anthropic", "bad\nmodel"));
        assert!(!h.orch.select_model("anthropic", &"x".repeat(201)));
        assert_eq!(h.orch.current_provider().unwrap().id, "anthropic");
    }

    #[tokio::test]
    async fn the_available_models_list_each_providers_default() {
        let h = harness(built_in_config());
        let list = h.orch.available_models().await;
        let pairs: Vec<(&str, &str)> = list
            .iter()
            .map(|c| (c.provider.as_str(), c.model.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("anthropic", "claude-haiku-5-5"),
                ("openai", "gpt-6-luna"),
                ("gemini", "gemini-3.5-flash-lite"),
                ("deepseek", "deepseek-flash"),
                ("chatgpt", "gpt-6-luna"),
            ],
            "Ollama contributes only what its server lists"
        );
        assert!(list[0].requires_key);
    }

    #[tokio::test]
    async fn the_connection_status_follows_the_stored_credentials() {
        let h = harness(built_in_config());
        let status = |list: &[ConnectionStatus], id: &str| {
            list.iter().find(|s| s.id == id).map(|s| s.connected)
        };
        let before = h.orch.connection_status().await;
        assert_eq!(status(&before, "anthropic"), Some(false));
        assert_eq!(
            status(&before, "ollama"),
            Some(true),
            "a local server needs no credential"
        );
        assert!(before.iter().find(|s| s.id == "chatgpt").unwrap().signs_in);

        h.orch
            .save_api_key("anthropic", Secret::new("a-key"))
            .await
            .unwrap();
        assert_eq!(
            status(&h.orch.connection_status().await, "anthropic"),
            Some(true)
        );

        h.orch.delete_api_key("anthropic").await.unwrap();
        assert_eq!(
            status(&h.orch.connection_status().await, "anthropic"),
            Some(false)
        );
    }

    #[test]
    fn model_lists_of_hosted_services_keep_only_chat_models() {
        for chat in [
            "gpt-6-luna",
            "claude-haiku-5-5",
            "gemini-3.5-flash-lite",
            "o4-mini",
        ] {
            assert!(chat_model(chat), "{chat}");
        }
        for other in [
            "text-embedding-3-small",
            "whisper-1",
            "gpt-4o-mini-tts",
            "dall-e-3",
            "omni-moderation-latest",
            "gpt-realtime",
            "gpt-image-1",
            "imagen-4",
        ] {
            assert!(!chat_model(other), "{other}");
        }
    }

    #[test]
    fn the_system_description_follows_the_users_own_instructions_unless_turned_off() {
        let both = compose_instructions(Some("Be brief."), true, false).unwrap();
        assert!(
            both.starts_with("Be brief.\n\nThe user's computer:"),
            "{both}"
        );
        assert!(
            compose_instructions(None, true, false)
                .unwrap()
                .starts_with("The user's computer:")
        );
        assert!(
            compose_instructions(None, true, true)
                .unwrap()
                .contains("run_command")
        );
        assert_eq!(compose_instructions(None, false, false), None);
        assert_eq!(compose_instructions(Some("  "), false, true), None);
        assert_eq!(
            compose_instructions(Some("Be brief."), false, true).as_deref(),
            Some("Be brief.")
        );
    }

    /// A provider that can use tools, whose turns are scripted by the test.
    struct AgentFake {
        turns: Mutex<VecDeque<Vec<AgentEvent>>>,
        requests: Mutex<Vec<AgentRequest>>,
    }

    impl Provider for AgentFake {
        fn kind(&self) -> &'static str {
            "agent-fake"
        }
        fn supports_tools(&self) -> bool {
            true
        }
        fn stream_chat(&self, _: ChatRequest) -> BoxFuture<'_, Result<ChatStream, ProviderError>> {
            Box::pin(async { Err(ProviderError::InvalidRequest("use tools".into())) })
        }
        fn stream_agent(
            &self,
            request: AgentRequest,
        ) -> BoxFuture<'_, Result<popupai_ai::AgentStream, ProviderError>> {
            self.requests.lock().unwrap().push(request);
            let turn = self.turns.lock().unwrap().pop_front().unwrap_or_default();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(turn.into_iter().map(Ok))) as popupai_ai::AgentStream)
            })
        }
        fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    struct AgentFactory(Arc<AgentFake>);

    impl ProviderFactory for AgentFactory {
        fn build(
            &self,
            _: &ProviderEntry,
            _: Endpoint,
            _: Option<Secret>,
            _: Duration,
        ) -> Result<Arc<dyn Provider>, ProviderError> {
            Ok(self.0.clone())
        }
    }

    fn call(id: &str, command: &str) -> AgentEvent {
        AgentEvent::Call(ToolCall {
            call_id: id.into(),
            name: crate::tools::TOOL_NAME.into(),
            arguments: serde_json::json!({ "command": command }).to_string(),
        })
    }

    fn agent_harness(
        turns: Vec<Vec<AgentEvent>>,
    ) -> (Orchestrator, UnboundedReceiver<Event>, Arc<AgentFake>) {
        let fake = Arc::new(AgentFake {
            turns: Mutex::new(turns.into()),
            requests: Mutex::new(Vec::new()),
        });
        let (orch, events) = Orchestrator::new(
            config(),
            Arc::new(MemoryStore::default()),
            Arc::new(AgentFactory(fake.clone())),
        )
        .unwrap();
        (orch, events, fake)
    }

    async fn collect_until_finished(events: &mut UnboundedReceiver<Event>) -> String {
        let mut text = String::new();
        loop {
            match next(events).await {
                Event::Chunk { text: t, .. } => text.push_str(&t),
                Event::Finished { .. } => return text,
                Event::Failed { message, .. } => panic!("failed: {message}"),
                Event::Permission { .. } => panic!("unexpected permission request"),
            }
        }
    }

    #[tokio::test]
    async fn a_read_only_command_runs_at_once_and_its_output_goes_back_to_the_model() {
        let (orch, mut events, fake) = agent_harness(vec![
            vec![AgentEvent::Text("Checking.".into()), call("c1", "uname")],
            vec![AgentEvent::Text("It is Linux.".into())],
        ]);
        orch.open();
        orch.submit("what system?").unwrap();
        let text = collect_until_finished(&mut events).await;
        assert!(text.contains("Checking."), "{text}");
        assert!(text.contains("`$ uname`"), "the command is shown: {text}");
        assert!(text.contains("It is Linux."), "{text}");

        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].tools.len(),
            1,
            "the model is offered the command tool"
        );
        assert_eq!(requests[0].tools[0].name, crate::tools::TOOL_NAME);
        let AgentItem::Output { output, .. } = requests[1].items.last().unwrap() else {
            panic!("the result goes back to the model");
        };
        assert!(output.starts_with("exit code 0"), "{output}");
        assert!(output.contains("Linux"), "{output}");
    }

    #[tokio::test]
    async fn a_command_that_is_not_a_plain_read_waits_for_the_user_and_can_be_refused() {
        let (orch, mut events, fake) = agent_harness(vec![
            vec![call("c1", "touch /tmp/popupai-should-not-exist")],
            vec![AgentEvent::Text("Understood.".into())],
        ]);
        orch.open();
        orch.submit("make a file").unwrap();
        let (call_id, command) = loop {
            match next(&mut events).await {
                Event::Permission {
                    call_id, command, ..
                } => break (call_id, command),
                Event::Chunk { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        };
        assert_eq!(command, "touch /tmp/popupai-should-not-exist");
        orch.answer_permission(&call_id, false);
        let text = collect_until_finished(&mut events).await;
        assert!(text.contains("not run"), "{text}");
        assert!(!std::path::Path::new("/tmp/popupai-should-not-exist").exists());
        let requests = fake.requests.lock().unwrap();
        let AgentItem::Output { output, .. } = requests[1].items.last().unwrap() else {
            panic!("the refusal goes back to the model");
        };
        assert!(output.contains("declined"), "{output}");
    }

    #[tokio::test]
    async fn an_approved_command_runs_and_its_output_goes_back() {
        let (orch, mut events, fake) = agent_harness(vec![
            vec![call("c1", "echo approved")],
            vec![AgentEvent::Text("Done.".into())],
        ]);
        orch.open();
        orch.submit("say it").unwrap();
        loop {
            match next(&mut events).await {
                Event::Permission { call_id, .. } => {
                    orch.answer_permission(&call_id, true);
                    break;
                }
                Event::Chunk { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        collect_until_finished(&mut events).await;
        let requests = fake.requests.lock().unwrap();
        let AgentItem::Output { output, .. } = requests[1].items.last().unwrap() else {
            panic!("the output goes back");
        };
        assert_eq!(output, "exit code 0\napproved\n");
    }

    #[tokio::test]
    async fn a_model_that_keeps_asking_is_stopped_after_a_few_steps() {
        let turns = (0..20)
            .map(|i| vec![call(&format!("c{i}"), "pwd")])
            .collect();
        let (orch, mut events, fake) = agent_harness(turns);
        orch.open();
        orch.submit("loop").unwrap();
        let text = collect_until_finished(&mut events).await;
        assert!(text.contains("too many steps"), "{text}");
        assert_eq!(fake.requests.lock().unwrap().len(), MAX_AGENT_STEPS);
    }

    #[tokio::test]
    async fn commands_are_not_offered_when_turned_off() {
        let mut settings = config();
        settings.advanced.allow_commands = false;
        let fake = Arc::new(AgentFake {
            turns: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        });
        let (orch, mut events) = Orchestrator::new(
            settings,
            Arc::new(MemoryStore::default()),
            Arc::new(AgentFactory(fake.clone())),
        )
        .unwrap();
        orch.open();
        orch.submit("hi").unwrap();
        // The scripted provider refuses plain chat, which is what shows it was used instead.
        let Event::Failed { .. } = next(&mut events).await else {
            panic!("the plain chat path is expected");
        };
        assert!(fake.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_mcp_tool_is_offered_and_every_call_asks_first() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mock_mcp.py");
        let server = crate::mcp::McpServer {
            id: "test/mock".into(),
            name: "mock".into(),
            source: "test",
            command: "python3".into(),
            args: vec!["-I".into(), script.into()],
            env: std::collections::BTreeMap::new(),
        };
        let (orch, mut events, fake) = agent_harness(vec![
            vec![call_tool("c1", "mcp__mock__echo", r#"{"text":"hi"}"#)],
            vec![AgentEvent::Text("Done.".into())],
        ]);
        orch.set_mcp_servers(vec![server]);
        orch.open();
        orch.submit("use the tool").unwrap();
        let command = loop {
            match next(&mut events).await {
                Event::Permission {
                    call_id,
                    command,
                    reason,
                    ..
                } => {
                    assert!(reason.contains("MCP server"), "{reason}");
                    orch.answer_permission(&call_id, true);
                    break command;
                }
                Event::Chunk { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        };
        assert!(command.starts_with("mock: echo("), "{command}");
        collect_until_finished(&mut events).await;
        let requests = fake.requests.lock().unwrap();
        let names: Vec<&str> = requests[0].tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["run_command", "mcp__mock__echo"]);
        let AgentItem::Output { output, .. } = requests[1].items.last().unwrap() else {
            panic!("the tool result goes back");
        };
        assert_eq!(output, "echo: hi");
    }

    fn call_tool(id: &str, name: &str, arguments: &str) -> AgentEvent {
        AgentEvent::Call(ToolCall {
            call_id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        })
    }

    #[test]
    fn mcp_tool_names_are_clean_and_bounded() {
        assert_eq!(mcp_tool_name("my server", "do.it"), "mcp__my_server__do_it");
        assert!(mcp_tool_name(&"s".repeat(80), "t").len() <= 64);
    }
}
