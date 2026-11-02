//! PopUpAI security primitives.
//!
//! Everything that decides whether a secret may be stored, logged or sent
//! lives here so it can be reviewed in one place. No UI and no OS dependencies.

mod endpoint;
mod redact;
mod secret;
mod store;

pub use endpoint::{Endpoint, EndpointError, HttpPolicy};
pub use redact::{is_sensitive_header, redact};
pub use secret::Secret;
pub use store::{MemoryStore, SecretStore, StoreError, UnavailableStore, validate_account};
