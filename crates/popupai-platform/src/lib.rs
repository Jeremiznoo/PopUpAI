//! PopUpAI desktop integration that does not need Qt.
//!
//! Single-instance detection and remote commands over D-Bus, the global shortcut
//! through the XDG desktop portal, and the system secret store. The tray icon and autostart land here next, behind
//! the same kind of small, testable interfaces.

mod ipc;
mod secret_service;
pub mod shortcut;

pub use secret_service::SecretServiceStore;

pub use ipc::{
    BUS_NAME, Bus, Claim, Command, Existing, INTERFACE, Instance, IpcError, OBJECT_PATH, claim,
};
