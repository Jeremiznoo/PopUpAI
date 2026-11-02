use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::Secret;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// No secure store can be used. Callers must tell the user and refuse to
    /// save the key; there is deliberately no plaintext fallback.
    #[error("no secure secret store is available: {0}")]
    Unavailable(String),
    #[error("secret store error: {0}")]
    Backend(String),
    #[error("invalid account name `{0}`")]
    InvalidAccount(String),
}

/// Where API keys live. Implementations wrap the operating system's secret
/// store (Secret Service or KWallet, Credential Manager, Keychain).
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<Secret>, StoreError>;
    fn set(&self, account: &str, secret: &Secret) -> Result<(), StoreError>;
    fn delete(&self, account: &str) -> Result<(), StoreError>;
}

/// Accounts are lowercase `[a-z0-9._-]`, 1 to 64 characters, so they are safe in
/// any backend's attribute or key syntax.
pub fn validate_account(account: &str) -> Result<(), StoreError> {
    let valid = (1..=64).contains(&account.len())
        && account.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(StoreError::InvalidAccount(
            account.chars().take(32).collect(),
        ))
    }
}

/// The store used when no secure backend could be opened. Every operation fails
/// closed so a key can never end up somewhere insecure.
#[derive(Debug, Clone)]
pub struct UnavailableStore {
    reason: String,
}

impl UnavailableStore {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    fn fail<T>(&self) -> Result<T, StoreError> {
        Err(StoreError::Unavailable(self.reason.clone()))
    }
}

impl SecretStore for UnavailableStore {
    fn get(&self, _account: &str) -> Result<Option<Secret>, StoreError> {
        self.fail()
    }

    fn set(&self, _account: &str, _secret: &Secret) -> Result<(), StoreError> {
        self.fail()
    }

    fn delete(&self, _account: &str) -> Result<(), StoreError> {
        self.fail()
    }
}

/// Process-memory store for tests. It persists nothing and must never be wired
/// in as a fallback when the real secret store is unavailable.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<HashMap<String, Secret>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Secret>, StoreError> {
        validate_account(account)?;
        let map = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(map.get(account).cloned())
    }

    fn set(&self, account: &str, secret: &Secret) -> Result<(), StoreError> {
        validate_account(account)?;
        let mut map = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        map.insert(account.to_owned(), secret.clone());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        validate_account(account)?;
        let mut map = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        map.remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_are_restricted() {
        for ok in ["openai", "provider.anthropic-1", "a_b", &"x".repeat(64)] {
            assert!(validate_account(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "Upper",
            "has space",
            "slash/name",
            "nul\0",
            &"x".repeat(65),
        ] {
            assert!(validate_account(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn memory_store_round_trips_and_deletes() {
        let store = MemoryStore::default();
        assert!(store.get("openai").unwrap().is_none());
        store.set("openai", &Secret::new("k1")).unwrap();
        assert_eq!(store.get("openai").unwrap().unwrap().expose(), "k1");
        store.set("openai", &Secret::new("k2")).unwrap();
        assert_eq!(store.get("openai").unwrap().unwrap().expose(), "k2");
        store.delete("openai").unwrap();
        assert!(store.get("openai").unwrap().is_none());
        assert!(store.set("BAD NAME", &Secret::new("x")).is_err());
    }

    #[test]
    fn unavailable_store_fails_closed_on_every_operation() {
        let store = UnavailableStore::new("no Secret Service on the session bus");
        let expected = StoreError::Unavailable("no Secret Service on the session bus".into());
        assert_eq!(store.get("openai").unwrap_err(), expected);
        assert_eq!(
            store.set("openai", &Secret::new("k")).unwrap_err(),
            expected
        );
        assert_eq!(store.delete("openai").unwrap_err(), expected);
    }

    #[test]
    fn trait_objects_work() {
        let stores: Vec<Box<dyn SecretStore>> = vec![
            Box::new(MemoryStore::default()),
            Box::new(UnavailableStore::new("x")),
        ];
        assert_eq!(stores.len(), 2);
    }
}
