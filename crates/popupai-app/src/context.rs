//! What the QML-created controller needs from the rest of the application.
//!
//! QML constructs the controller itself, so it cannot receive arguments; it reads
//! this context, installed once in `main` before the QML engine starts.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use popupai_core::orchestrator::{Event, Orchestrator};
use popupai_platform::Command;
use popupai_platform::shortcut::ShortcutStatus;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Everything `main` hands over when it builds the context.
pub struct Parts {
    pub orchestrator: Orchestrator,
    pub runtime: Handle,
    /// A problem to show to the user at start-up (for example an unreadable
    /// configuration file). Empty when there is none.
    pub notice: String,
    /// Where the favourite and recent models are kept.
    pub state_path: PathBuf,
    /// Whether the popup shows as soon as the application starts.
    pub start_visible: bool,
    /// Development aid: quit when the popup is dismissed instead of staying
    /// resident (`POPUPAI_EXIT_ON_DISMISS=1`).
    pub exit_on_dismiss: bool,
    pub events: UnboundedReceiver<Event>,
    /// Commands from later invocations (`--toggle`, `--quit`) and from the global
    /// shortcut.
    pub commands: Option<UnboundedReceiver<Command>>,
    /// What the portal says about the global shortcut, when one was requested.
    pub shortcut: Option<UnboundedReceiver<ShortcutStatus>>,
    /// Asks the shortcut task to open the desktop's shortcut settings.
    pub shortcut_configure: Option<UnboundedSender<()>>,
}

pub struct AppContext {
    pub orchestrator: Orchestrator,
    pub runtime: Handle,
    pub notice: String,
    pub state_path: PathBuf,
    pub start_visible: bool,
    pub exit_on_dismiss: bool,
    pub shortcut_configure: Option<UnboundedSender<()>>,
    events: Mutex<Option<UnboundedReceiver<Event>>>,
    commands: Mutex<Option<UnboundedReceiver<Command>>>,
    shortcut: Mutex<Option<UnboundedReceiver<ShortcutStatus>>>,
}

impl AppContext {
    pub fn new(parts: Parts) -> Self {
        Self {
            orchestrator: parts.orchestrator,
            runtime: parts.runtime,
            notice: parts.notice,
            state_path: parts.state_path,
            start_visible: parts.start_visible,
            exit_on_dismiss: parts.exit_on_dismiss,
            shortcut_configure: parts.shortcut_configure,
            events: Mutex::new(Some(parts.events)),
            commands: Mutex::new(parts.commands),
            shortcut: Mutex::new(parts.shortcut),
        }
    }

    /// The event stream can be taken once, by the controller.
    pub fn take_events(&self) -> Option<UnboundedReceiver<Event>> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// The shortcut status stream can be taken once, by the controller.
    pub fn take_shortcut(&self) -> Option<UnboundedReceiver<ShortcutStatus>> {
        self.shortcut
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// The command stream can be taken once, by the controller.
    pub fn take_commands(&self) -> Option<UnboundedReceiver<Command>> {
        self.commands
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
