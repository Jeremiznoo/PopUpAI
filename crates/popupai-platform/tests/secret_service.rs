//! The Secret Service store.
//!
//! The first test runs on a private bus where nothing provides the service, so it
//! never reaches the user's wallet. The second talks to the **real** Secret Service of
//! the current session (KDE Wallet on Plasma) and is ignored by default: it stores and
//! removes one throw-away entry, which can make the wallet ask to be unlocked. Run it
//! on purpose with:
//!
//! ```text
//! cargo test -p popupai-platform --test secret_service -- --ignored
//! ```

mod support;

use popupai_platform::SecretServiceStore;
use popupai_security::{Secret, SecretStore, StoreError};
use support::PrivateBus;

#[test]
fn without_a_secret_service_every_operation_fails_closed() {
    let bus = PrivateBus::start();
    let store = SecretServiceStore::on_bus(bus.address.clone());

    for result in [
        store.get("provider-anthropic").map(|_| ()),
        store.set("provider-anthropic", &Secret::new("sk-test")),
        store.delete("provider-anthropic"),
    ] {
        match result {
            Err(StoreError::Unavailable(reason)) => {
                assert!(reason.contains("Secret Service"), "{reason}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }
}

#[test]
#[ignore = "uses the real Secret Service of the current session"]
fn the_real_secret_service_stores_replaces_reads_and_deletes_a_secret() {
    let store = SecretServiceStore::new();
    let account = format!("selftest-{}", std::process::id());

    assert!(store.get(&account).expect("read").is_none());

    store
        .set(&account, &Secret::new("first-value"))
        .expect("write");
    let read = store
        .get(&account)
        .expect("read")
        .expect("the secret exists");
    assert_eq!(read.expose(), "first-value");

    store
        .set(&account, &Secret::new("second-value"))
        .expect("replace");
    let read = store
        .get(&account)
        .expect("read")
        .expect("the secret exists");
    assert_eq!(
        read.expose(),
        "second-value",
        "set replaces, it does not add"
    );

    store.delete(&account).expect("delete");
    assert!(store.get(&account).expect("read").is_none());
    // Deleting what is gone is not an error.
    store.delete(&account).expect("delete twice");
}
