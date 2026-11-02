//! PopUpAI desktop integration that does not need Qt.
//!
//! Single-instance detection and remote commands over D-Bus, and the global shortcut
//! through the XDG desktop portal. The tray icon and autostart land here next, behind
//! the same kind of small, testable interfaces.

mod ipc;
pub mod shortcut;

pub use ipc::{
    BUS_NAME, Bus, Claim, Command, Existing, INTERFACE, Instance, IpcError, OBJECT_PATH, claim,
};
