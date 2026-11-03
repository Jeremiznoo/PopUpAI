//! PopUpAI application core.
//!
//! Application logic with no UI and no operating-system dependencies, so every
//! rule can be unit-tested without a desktop.

pub mod config;
pub mod credentials;
pub mod markdown;
pub mod orchestrator;
pub mod request_error;
pub mod session;
pub mod shortcut;
pub mod state;
pub mod system_info;
