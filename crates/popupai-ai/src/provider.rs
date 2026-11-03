use std::time::Duration;

use futures_util::future::BoxFuture;
use popupai_security::{Endpoint, Secret};
use serde_json::Value;

use crate::{ChatRequest, ChatStream, ModelInfo, ProviderError};

/// How long to wait to connect, and for each further piece of data. It is not a
/// limit on the whole answer. Slow local models may need a generous value.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Connection settings for one provider. The key is a [`Secret`], so deriving
/// `Debug` cannot leak it.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub endpoint: Endpoint,
    pub api_key: Option<Secret>,
    pub timeout: Duration,
}

impl ProviderConfig {
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            api_key: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_api_key(mut self, key: Secret) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// What every provider adapter offers. Implementations are independent: an
/// error in one cannot affect another.
pub trait Provider: Send + Sync {
    /// Short identifier for logs and settings, such as `"anthropic"`.
    fn kind(&self) -> &'static str;

    /// Start a streamed completion. Dropping the returned stream (or this
    /// future before it completes) aborts the HTTP request, which is how
    /// cancellation works.
    fn stream_chat(&self, request: ChatRequest)
    -> BoxFuture<'_, Result<ChatStream, ProviderError>>;

    /// Whether [`Provider::stream_agent`] works: the provider can be given tools.
    fn supports_tools(&self) -> bool {
        false
    }

    /// A streamed completion in which the model may ask for tools to be run. The caller runs
    /// them and calls this again with the results appended to `items`.
    fn stream_agent(
        &self,
        _request: crate::AgentRequest,
    ) -> BoxFuture<'_, Result<crate::AgentStream, ProviderError>> {
        Box::pin(async {
            Err(ProviderError::InvalidRequest(
                "this provider cannot run tools".into(),
            ))
        })
    }

    /// List available models. Also serves as the connection test.
    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>>;
}

/// Collect `field` from every object in a JSON array, sorted and de-duplicated.
pub(crate) fn collect_ids(list: Option<&Value>, field: &str) -> Vec<ModelInfo> {
    let mut models: Vec<ModelInfo> = list
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(field).and_then(Value::as_str))
        .map(|id| ModelInfo { id: id.to_owned() })
        .collect();
    models.sort();
    models.dedup();
    models
}
