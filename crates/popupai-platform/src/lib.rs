//! PopUpAI desktop integration that does not need Qt.
//!
//! For now: single-instance detection and remote commands over D-Bus. The global
//! shortcut (XDG portal), the tray icon and autostart land here next, behind the same
//! kind of small, testable interfaces.

mod ipc;

pub use ipc::{
    BUS_NAME, Bus, Claim, Command, Existing, INTERFACE, Instance, IpcError, OBJECT_PATH, claim,
};
