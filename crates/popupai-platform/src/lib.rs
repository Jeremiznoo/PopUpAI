//! PopUpAI desktop integration that does not need Qt.
//!
//! Single-instance detection and remote commands over D-Bus, the global shortcut
//! through the XDG desktop portal, the tray icon, and the system secret store. The tray icon and autostart land here next, behind
//! the same kind of small, testable interfaces.

mod autostart;
mod ipc;
mod secret_service;
pub mod shortcut;
pub mod tray;

pub use autostart::Autostart;
pub use secret_service::SecretServiceStore;

pub use ipc::{
    BUS_NAME, Bus, Claim, Command, Existing, INTERFACE, Instance, IpcError, OBJECT_PATH, claim,
};
