//! The system secret store, through the freedesktop Secret Service API.
//!
//! On KDE Plasma that is KDE Wallet (`ksecretd`), on GNOME it is GNOME Keyring, and
//! KeePassXC can provide it too. API keys are kept there and nowhere else: when the
//! service is missing every operation fails (see [`StoreError::Unavailable`]) instead
//! of falling back to a file.
//!
//! The session with the service is encrypted (Diffie-Hellman), so a secret never
//! crosses the bus in the clear. Each call opens its own connection: keys are read
//! once per provider and cached by the orchestrator, so the cost is paid rarely and
//! no long-lived handle on the wallet is kept.

use std::collections::HashMap;

use popupai_security::{Secret, SecretStore, StoreError, validate_account};
use secret_service::blocking::SecretService;
use secret_service::{EncryptionType, Error};
use zeroize::Zeroizing;

const APPLICATION: &str = "popupai";
const NO_SERVICE: &str = "no Secret Service is running (KDE Wallet, GNOME Keyring or KeePassXC with its Secret Service integration)";

pub struct SecretServiceStore {
    /// `None` means the user's session bus. A private bus address is for tests, so
    /// they never reach the real wallet.
    bus: Option<String>,
}

impl SecretServiceStore {
    pub fn new() -> Self {
        Self { bus: None }
    }

    pub fn on_bus(address: impl Into<String>) -> Self {
        Self {
            bus: Some(address.into()),
        }
    }

    fn connect(&self) -> Result<SecretService<'static>, StoreError> {
        match &self.bus {
            None => SecretService::connect(EncryptionType::Dh),
            Some(address) => {
                let connection = zbus::blocking::connection::Builder::address(address.as_str())
                    .and_then(zbus::blocking::connection::Builder::build)
                    .map_err(|err| {
                        StoreError::Unavailable(format!("cannot reach the bus: {err}"))
                    })?;
                SecretService::connect_with_existing(EncryptionType::Dh, connection)
            }
        }
        .map_err(map_error)
    }
}

impl Default for SecretServiceStore {
    fn default() -> Self {
        Self::new()
    }
}

fn is_missing_service(err: &zbus::Error) -> bool {
    matches!(
        err,
        zbus::Error::MethodError(name, ..)
            if name.as_str().ends_with("ServiceUnknown") || name.as_str().ends_with("NameHasNoOwner")
    ) || matches!(err, zbus::Error::InputOutput(_))
}

fn map_error(err: Error) -> StoreError {
    match err {
        Error::Unavailable => StoreError::Unavailable(NO_SERVICE.to_owned()),
        Error::Zbus(ref inner) if is_missing_service(inner) => {
            StoreError::Unavailable(NO_SERVICE.to_owned())
        }
        Error::Locked | Error::Prompt => StoreError::Backend(
            "the wallet is locked and the request to unlock it was dismissed".to_owned(),
        ),
        other => StoreError::Backend(other.to_string()),
    }
}

fn attributes(account: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", APPLICATION), ("account", account)])
}

impl SecretStore for SecretServiceStore {
    fn get(&self, account: &str) -> Result<Option<Secret>, StoreError> {
        validate_account(account)?;
        let service = self.connect()?;
        let found = service
            .search_items(attributes(account))
            .map_err(map_error)?;
        if !found.locked.is_empty() {
            let locked: Vec<_> = found.locked.iter().collect();
            service.unlock_all(&locked).map_err(map_error)?;
        }
        let Some(item) = found.unlocked.iter().chain(found.locked.iter()).next() else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(item.get_secret().map_err(map_error)?);
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| StoreError::Backend("the stored secret is not valid text".to_owned()))?;
        Ok(Some(Secret::new(text)))
    }

    fn set(&self, account: &str, secret: &Secret) -> Result<(), StoreError> {
        validate_account(account)?;
        let service = self.connect()?;
        let collection = service.get_default_collection().map_err(map_error)?;
        collection.unlock().map_err(map_error)?;
        collection
            .create_item(
                &format!("PopUpAI ({account})"),
                attributes(account),
                secret.expose().as_bytes(),
                true, // replace the item with the same attributes
                "text/plain",
            )
            .map_err(map_error)?;
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), StoreError> {
        validate_account(account)?;
        let service = self.connect()?;
        let found = service
            .search_items(attributes(account))
            .map_err(map_error)?;
        if !found.locked.is_empty() {
            let locked: Vec<_> = found.locked.iter().collect();
            service.unlock_all(&locked).map_err(map_error)?;
        }
        for item in found.unlocked.iter().chain(found.locked.iter()) {
            item.delete().map_err(map_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_service_is_reported_as_unavailable() {
        assert!(matches!(
            map_error(Error::Unavailable),
            StoreError::Unavailable(_)
        ));
        let missing = zbus::Error::MethodError(
            "org.freedesktop.DBus.Error.ServiceUnknown"
                .try_into()
                .unwrap(),
            None,
            zbus::message::Message::method_call("/", "Ping")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        assert!(matches!(
            map_error(Error::Zbus(missing)),
            StoreError::Unavailable(_)
        ));
    }

    #[test]
    fn a_dismissed_unlock_prompt_is_a_backend_error_with_a_clear_text() {
        for err in [Error::Locked, Error::Prompt] {
            let StoreError::Backend(text) = map_error(err) else {
                panic!("expected a backend error");
            };
            assert!(text.contains("locked"), "{text}");
        }
    }

    #[test]
    fn invalid_account_names_are_refused_before_any_bus_call() {
        let store = SecretServiceStore::new();
        for account in ["", "UPPER", "has space", "a/b"] {
            assert!(matches!(
                store.get(account),
                Err(StoreError::InvalidAccount(_))
            ));
            assert!(matches!(
                store.set(account, &Secret::new("x")),
                Err(StoreError::InvalidAccount(_))
            ));
            assert!(matches!(
                store.delete(account),
                Err(StoreError::InvalidAccount(_))
            ));
        }
    }

    #[test]
    fn the_item_attributes_identify_the_application_and_the_account() {
        let attrs = attributes("provider-anthropic");
        assert_eq!(attrs.get("application"), Some(&"popupai"));
        assert_eq!(attrs.get("account"), Some(&"provider-anthropic"));
        assert_eq!(attrs.len(), 2);
    }
}
