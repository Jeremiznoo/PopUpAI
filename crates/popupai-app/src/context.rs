//! What the QML-created controller needs from the rest of the application.
//!
//! QML constructs the controller itself, so it cannot receive arguments; it reads
//! this context, installed once in `main` before the QML engine starts.

use std::sync::{Mutex, OnceLock, PoisonError};

use popupai_core::orchestrator::{Event, Orchestrator};
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;

pub struct AppContext {
    pub orchestrator: Orchestrator,
    pub runtime: Handle,
    /// A problem to show to the user at start-up (for example an unreadable
    /// configuration file). Empty when there is none.
    pub notice: String,
    events: Mutex<Option<UnboundedReceiver<Event>>>,
}

impl AppContext {
    pub fn new(
        orchestrator: Orchestrator,
        runtime: Handle,
        notice: String,
        events: UnboundedReceiver<Event>,
    ) -> Self {
        Self {
            orchestrator,
            runtime,
            notice,
            events: Mutex::new(Some(events)),
        }
    }

    /// The event stream can be taken once, by the controller.
    pub fn take_events(&self) -> Option<UnboundedReceiver<Event>> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

static CONTEXT: OnceLock<AppContext> = OnceLock::new();

/// Returns `false` if a context was already installed.
pub fn install(context: AppContext) -> bool {
    CONTEXT.set(context).is_ok()
}

pub fn get() -> Option<&'static AppContext> {
    CONTEXT.get()
}
