//! A stand-in for the desktop portal, serving just enough of the XDG protocol on a
//! private bus to drive the shortcut client end to end.
//!
//! It is written from the portal specification (Registry, GlobalShortcuts, Request),
//! **not** from KDE's implementation. Passing against it shows that the client speaks
//! the protocol and handles each reply; it says nothing about how KDE behaves, which
//! only a run on a real Plasma session can show.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Type, as_value};
use zbus::{Connection, connection, interface};

const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS_INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

/// How the portal answers `BindShortcuts`.
#[derive(Debug, Clone)]
pub enum Bind {
    /// The shortcut is bound; the portal describes the trigger the user ended up with.
    Grant { trigger: String },
    /// Bound, but no key is assigned (a conflict, or the user cleared it).
    NoTrigger,
    /// The user dismissed the dialog.
    Cancelled,
    /// A reply that does not contain the shortcut that was asked for.
    NoShortcut,
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Serve `org.freedesktop.host.portal.Registry`.
    pub registry: bool,
    /// Serve `org.freedesktop.portal.GlobalShortcuts`.
    pub global_shortcuts: bool,
    pub bind: Bind,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            registry: true,
            global_shortcuts: true,
            bind: Bind::Grant {
                trigger: "Ctrl+Shift+Space".to_owned(),
            },
        }
    }
}

#[derive(Default)]
struct State {
    app_ids: Mutex<Vec<String>>,
    /// Shortcut id and preferred trigger, for each shortcut asked for.
    bound: Mutex<Vec<(String, Option<String>)>>,
    session: Mutex<Option<String>>,
    configure_calls: Mutex<usize>,
}

#[derive(Serialize, Type)]
#[zvariant(signature = "dict")]
struct CreateResults {
    #[serde(with = "as_value")]
    session_handle: String,
}

#[derive(Serialize, Type)]
#[zvariant(signature = "dict")]
struct ShortcutInfo {
    #[serde(with = "as_value")]
    description: String,
    #[serde(with = "as_value")]
    trigger_description: String,
}

#[derive(Serialize, Type)]
struct ShortcutEntry(String, ShortcutInfo);

#[derive(Serialize, Type)]
#[zvariant(signature = "dict")]
struct BindResults {
    #[serde(with = "as_value")]
    shortcuts: Vec<ShortcutEntry>,
}

fn entry(id: &str, trigger: &str) -> ShortcutEntry {
    ShortcutEntry(
        id.to_owned(),
        ShortcutInfo {
            description: "mock".to_owned(),
            trigger_description: trigger.to_owned(),
        },
    )
}

struct Registry(Arc<State>);

#[interface(name = "org.freedesktop.host.portal.Registry")]
impl Registry {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    fn register(&self, app_id: String, _options: HashMap<String, OwnedValue>) {
        self.0.app_ids.lock().unwrap().push(app_id);
    }
}

struct Shortcuts {
    state: Arc<State>,
    bind: Bind,
}

fn text(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options
        .get(key)?
        .downcast_ref::<&str>()
        .ok()
        .map(str::to_owned)
}

/// The caller's unique name as it appears in request and session paths.
fn sender_segment(header: &Header<'_>) -> String {
    header
        .sender()
        .expect("a method call has a sender")
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_")
}

/// Answer a request the way the portal does: reply with the request handle at once,
/// then emit `Response` on it a little later.
fn respond<B>(connection: &Connection, request_path: String, code: u32, results: B)
where
    B: Serialize + Type + Send + Sync + 'static,
{
    let connection = connection.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        let _ = connection
            .emit_signal(
                None::<&str>,
                request_path,
                "org.freedesktop.portal.Request",
                "Response",
                &(code, results),
            )
            .await;
    });
}

#[interface(name = "org.freedesktop.portal.GlobalShortcuts")]
impl Shortcuts {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }

    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = sender_segment(&header);
        let handle = text(&options, "handle_token")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("handle_token is missing".to_owned()))?;
        let session_token = text(&options, "session_handle_token").ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs("session_handle_token is missing".to_owned())
        })?;
        let request = format!("{DESKTOP_PATH}/request/{sender}/{handle}");
        let session = format!("{DESKTOP_PATH}/session/{sender}/{session_token}");
        *self.state.session.lock().unwrap() = Some(session.clone());
        respond(
            connection,
            request.clone(),
            0,
            CreateResults {
                session_handle: session,
            },
        );
        Ok(OwnedObjectPath::try_from(request).expect("a valid path"))
    }

    fn configure_shortcuts(
        &self,
        _session_handle: OwnedObjectPath,
        _parent_window: String,
        _options: HashMap<String, OwnedValue>,
    ) {
        *self.state.configure_calls.lock().unwrap() += 1;
    }

    async fn bind_shortcuts(
        &self,
        _session_handle: OwnedObjectPath,
        shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
        _parent_window: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = sender_segment(&header);
        let handle = text(&options, "handle_token")
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("handle_token is missing".to_owned()))?;
        let request = format!("{DESKTOP_PATH}/request/{sender}/{handle}");

        let asked: Vec<(String, Option<String>)> = shortcuts
            .iter()
            .map(|(id, info)| (id.clone(), text(info, "preferred_trigger")))
            .collect();
        self.state.bound.lock().unwrap().extend(asked.clone());

        match &self.bind {
            Bind::Cancelled => respond(
                connection,
                request.clone(),
                1,
                HashMap::<String, OwnedValue>::new(),
            ),
            Bind::NoShortcut => respond(
                connection,
                request.clone(),
                0,
                BindResults { shortcuts: vec![] },
            ),
            Bind::Grant { trigger } => respond(
                connection,
                request.clone(),
                0,
                BindResults {
                    shortcuts: asked.iter().map(|(id, _)| entry(id, trigger)).collect(),
                },
            ),
            Bind::NoTrigger => respond(
                connection,
                request.clone(),
                0,
                BindResults {
                    shortcuts: asked.iter().map(|(id, _)| entry(id, "")).collect(),
                },
            ),
        }
        Ok(OwnedObjectPath::try_from(request).expect("a valid path"))
    }
}

pub struct MockPortal {
    connection: Connection,
    state: Arc<State>,
}

impl MockPortal {
    /// Own `org.freedesktop.portal.Desktop` on the bus at `address`.
    pub async fn start(address: &str, options: Options) -> Self {
        let state = Arc::new(State::default());
        let mut builder = connection::Builder::address(address)
            .expect("a valid bus address")
            .name("org.freedesktop.portal.Desktop")
            .expect("a valid name");
        if options.registry {
            builder = builder
                .serve_at(DESKTOP_PATH, Registry(state.clone()))
                .expect("serve the registry");
        }
        if options.global_shortcuts {
            builder = builder
                .serve_at(
                    DESKTOP_PATH,
                    Shortcuts {
                        state: state.clone(),
                        bind: options.bind.clone(),
                    },
                )
                .expect("serve the shortcuts");
        }
        let connection = builder.build().await.expect("start the mock portal");
        Self { connection, state }
    }

    /// How many times the client asked to show the shortcut settings.
    pub fn configure_calls(&self) -> usize {
        *self.state.configure_calls.lock().unwrap()
    }

    pub fn registered_app_ids(&self) -> Vec<String> {
        self.state.app_ids.lock().unwrap().clone()
    }

    /// Every shortcut asked for, with its preferred trigger.
    pub fn bound(&self) -> Vec<(String, Option<String>)> {
        self.state.bound.lock().unwrap().clone()
    }

    fn session(&self) -> OwnedObjectPath {
        let path = self
            .state
            .session
            .lock()
            .unwrap()
            .clone()
            .expect("a session was created");
        OwnedObjectPath::try_from(path).expect("a valid path")
    }

    /// The user pressed the shortcut.
    pub async fn activate(&self, shortcut_id: &str) {
        self.connection
            .emit_signal(
                None::<&str>,
                DESKTOP_PATH,
                SHORTCUTS_INTERFACE,
                "Activated",
                &(
                    self.session(),
                    shortcut_id,
                    1_000_u64,
                    HashMap::<String, OwnedValue>::new(),
                ),
            )
            .await
            .expect("emit Activated");
    }

    /// The user changed the shortcut in the system settings.
    pub async fn change_trigger(&self, shortcut_id: &str, trigger: &str) {
        self.connection
            .emit_signal(
                None::<&str>,
                DESKTOP_PATH,
                SHORTCUTS_INTERFACE,
                "ShortcutsChanged",
                &(self.session(), vec![entry(shortcut_id, trigger)]),
            )
            .await
            .expect("emit ShortcutsChanged");
    }

    /// The portal ends the session.
    pub async fn close_session(&self) {
        let session = self.session();
        self.connection
            .emit_signal(
                None::<&str>,
                ObjectPath::from(&session),
                "org.freedesktop.portal.Session",
                "Closed",
                &HashMap::<String, OwnedValue>::new(),
            )
            .await
            .expect("emit Closed");
    }
}
