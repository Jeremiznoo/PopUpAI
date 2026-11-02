//! API keys, bound to the endpoint they were saved for.
//!
//! The configuration file is not secret and could be tampered with. If someone
//! changed a provider's endpoint there, its key would be sent to the new host.
//! To prevent that, each key is stored together with the origin (scheme, host
//! and port) it was saved for and is only released for that same origin.
//! Pointing a provider somewhere else means saving the key again, which is the
//! user's explicit confirmation.

use popupai_security::{Secret, SecretStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the endpoint for `{provider}` changed since its API key was saved")]
    OriginChanged { provider: String },
    #[error("the API key is empty")]
    EmptyKey,
    #[error("there is no provider called `{0}`")]
    UnknownProvider(String),
    #[error("the provider's endpoint is not valid: {0}")]
    InvalidEndpoint(String),
}

/// Account names fit the `[a-z0-9._-]{1,64}` rule of the secret store: a
/// provider id has at most 48 characters, so both names stay within 64.
pub fn key_account(provider_id: &str) -> String {
    format!("provider-{provider_id}")
}

pub fn origin_account(provider_id: &str) -> String {
    format!("provider-{provider_id}.origin")
}

/// Store a key and remember which origin it belongs to. Surrounding whitespace
/// from a paste is dropped. A key is never left without its origin record.
pub fn save_key(
    store: &dyn SecretStore,
    provider_id: &str,
    origin: &str,
    key: &Secret,
) -> Result<(), CredentialError> {
    let key = Secret::new(key.expose().trim());
    if key.is_empty() {
        return Err(CredentialError::EmptyKey);
    }
    store.set(&key_account(provider_id), &key)?;
    if let Err(err) = store.set(&origin_account(provider_id), &Secret::new(origin)) {
        let _ = store.delete(&key_account(provider_id));
        return Err(err.into());
    }
    Ok(())
}

/// The key for `provider_id`, if there is one and it was saved for
/// `current_origin`. A key whose origin record is missing or different is
/// refused, so it can never reach another host.
pub fn load_key(
    store: &dyn SecretStore,
    provider_id: &str,
    current_origin: &str,
) -> Result<Option<Secret>, CredentialError> {
    let Some(key) = store.get(&key_account(provider_id))? else {
        return Ok(None);
    };
    match store.get(&origin_account(provider_id))? {
        Some(saved) if saved.expose() == current_origin => Ok(Some(key)),
        _ => Err(CredentialError::OriginChanged {
            provider: provider_id.to_owned(),
        }),
    }
}

pub fn has_key(store: &dyn SecretStore, provider_id: &str) -> Result<bool, CredentialError> {
    Ok(store.get(&key_account(provider_id))?.is_some())
}

/// Remove the key and its origin record. Both deletions are attempted.
pub fn delete_key(store: &dyn SecretStore, provider_id: &str) -> Result<(), CredentialError> {
    let key = store.delete(&key_account(provider_id));
    let origin = store.delete(&origin_account(provider_id));
    key.and(origin)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use popupai_security::{MemoryStore, UnavailableStore};

    use super::*;

    const ORIGIN: &str = "https://api.openai.com";

    fn key(value: &str) -> Secret {
        Secret::new(value)
    }

    #[test]
    fn a_key_is_released_only_for_the_origin_it_was_saved_for() {
        let store = MemoryStore::default();
        assert_eq!(
            load_key(&store, "openai", ORIGIN)
                .unwrap()
                .map(|k| k.expose().to_owned()),
            None
        );

        save_key(&store, "openai", ORIGIN, &key("k-1")).unwrap();
        assert_eq!(
            load_key(&store, "openai", ORIGIN)
                .unwrap()
                .unwrap()
                .expose(),
            "k-1"
        );

        let elsewhere = load_key(&store, "openai", "https://evil.example");
        assert_eq!(
            elsewhere.unwrap_err(),
            CredentialError::OriginChanged {
                provider: "openai".into()
            }
        );
        let other_port = load_key(&store, "openai", "https://api.openai.com:8443");
        assert!(matches!(
            other_port,
            Err(CredentialError::OriginChanged { .. })
        ));
    }

    #[test]
    fn saving_again_confirms_a_new_origin() {
        let store = MemoryStore::default();
        save_key(&store, "p", ORIGIN, &key("k-1")).unwrap();
        save_key(&store, "p", "https://proxy.example", &key("k-2")).unwrap();
        assert_eq!(
            load_key(&store, "p", "https://proxy.example")
                .unwrap()
                .unwrap()
                .expose(),
            "k-2"
        );
        assert!(load_key(&store, "p", ORIGIN).is_err());
    }

    #[test]
    fn a_key_without_an_origin_record_is_not_trusted() {
        let store = MemoryStore::default();
        store.set(&key_account("p"), &key("planted")).unwrap();
        assert!(matches!(
            load_key(&store, "p", ORIGIN),
            Err(CredentialError::OriginChanged { .. })
        ));
    }

    #[test]
    fn keys_are_trimmed_and_empty_keys_rejected() {
        let store = MemoryStore::default();
        save_key(&store, "p", ORIGIN, &key("  k-1\n")).unwrap();
        assert_eq!(
            load_key(&store, "p", ORIGIN).unwrap().unwrap().expose(),
            "k-1"
        );
        assert_eq!(
            save_key(&store, "p", ORIGIN, &key(" \n")).unwrap_err(),
            CredentialError::EmptyKey
        );
    }

    #[test]
    fn providers_do_not_see_each_others_keys() {
        let store = MemoryStore::default();
        save_key(&store, "a", ORIGIN, &key("key-a")).unwrap();
        save_key(&store, "b", ORIGIN, &key("key-b")).unwrap();
        assert_eq!(
            load_key(&store, "a", ORIGIN).unwrap().unwrap().expose(),
            "key-a"
        );
        assert_eq!(
            load_key(&store, "b", ORIGIN).unwrap().unwrap().expose(),
            "key-b"
        );
        assert!(has_key(&store, "a").unwrap());
        assert!(!has_key(&store, "c").unwrap());
    }

    #[test]
    fn deleting_removes_the_key_and_its_origin() {
        let store = MemoryStore::default();
        save_key(&store, "p", ORIGIN, &key("k")).unwrap();
        delete_key(&store, "p").unwrap();
        assert!(!has_key(&store, "p").unwrap());
        assert!(store.get(&origin_account("p")).unwrap().is_none());
        delete_key(&store, "p").unwrap();
    }

    #[test]
    fn an_unavailable_store_blocks_everything_without_a_plaintext_fallback() {
        let store = UnavailableStore::new("no Secret Service");
        let expected = CredentialError::Store(StoreError::Unavailable("no Secret Service".into()));
        assert_eq!(
            save_key(&store, "p", ORIGIN, &key("k")).unwrap_err(),
            expected
        );
        assert_eq!(load_key(&store, "p", ORIGIN).unwrap_err(), expected);
        assert_eq!(delete_key(&store, "p").unwrap_err(), expected);
    }

    /// Fails to write origin records, to prove a key is never left unbound.
    #[derive(Default)]
    struct NoOriginWrites(MemoryStore);

    impl SecretStore for NoOriginWrites {
        fn get(&self, account: &str) -> Result<Option<Secret>, StoreError> {
            self.0.get(account)
        }
        fn set(&self, account: &str, secret: &Secret) -> Result<(), StoreError> {
            if account.ends_with(".origin") {
                return Err(StoreError::Backend("disk full".into()));
            }
            self.0.set(account, secret)
        }
        fn delete(&self, account: &str) -> Result<(), StoreError> {
            self.0.delete(account)
        }
    }

    #[test]
    fn a_failed_origin_write_rolls_the_key_back() {
        let store = NoOriginWrites::default();
        assert!(save_key(&store, "p", ORIGIN, &key("k")).is_err());
        assert!(
            store.get(&key_account("p")).unwrap().is_none(),
            "no unbound key may remain"
        );
    }
}
