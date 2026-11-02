//! The global shortcut, through the XDG Desktop Portal `GlobalShortcuts` interface.
//!
//! The application asks the portal for one shortcut (`toggle`) with a preferred
//! trigger, then waits for its `Activated` signal. On KDE the portal backend hands the
//! request to KGlobalAccel, which keeps the final trigger: the user, not the
//! application, has the last word (ADR 0002, decision 4).
//!
//! Nothing here falls back silently. When the portal cannot be used the task reports
//! [`ShortcutStatus::Unavailable`] with a reason, and the application shows it.

use ashpd::desktop::ResponseError;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut};
use ashpd::{AppID, Error, PortalError, register_host_app_with_connection};
use futures_util::StreamExt;
use popupai_core::shortcut::{Chord, Key, NamedKey};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use crate::ipc::{Bus, Command, connect};

/// The portal-side identifier of the one shortcut the application registers.
pub const SHORTCUT_ID: &str = "toggle";

const DESCRIPTION: &str = "Show or hide the PopUpAI quick-ask popup";

/// What the portal said about the shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutStatus {
    /// The shortcut is bound. `trigger` is the portal's own, human-readable
    /// description of the key the user ended up with; it may differ from the
    /// preferred one (a conflict, or a choice made in the system settings).
    Active {
        trigger: String,
        /// Set when the application could not register its id with the portal. The
        /// shortcut may still work, but under an identity the portal worked out
        /// itself, so it is worth showing.
        registration: Option<String>,
    },
    /// The portal knows the shortcut but no key is assigned to it.
    Unassigned,
    /// The shortcut could not be set up, or stopped working. The text is meant to be
    /// shown to the user.
    Unavailable { reason: String },
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// The application id: names the application to the portal, and so names the
    /// entry in the system's shortcut settings.
    pub app_id: String,
    pub chord: Chord,
}

/// The XKB keysym name for a named key, as the XDG shortcuts specification wants it.
fn xkb_name(key: NamedKey) -> &'static str {
    match key {
        NamedKey::Space => "space",
        NamedKey::Enter => "Return",
        NamedKey::Tab => "Tab",
        NamedKey::Escape => "Escape",
        NamedKey::Backspace => "BackSpace",
        NamedKey::Delete => "Delete",
        NamedKey::Up => "Up",
        NamedKey::Down => "Down",
        NamedKey::Left => "Left",
        NamedKey::Right => "Right",
        NamedKey::Home => "Home",
        NamedKey::End => "End",
        NamedKey::PageUp => "Page_Up",
        NamedKey::PageDown => "Page_Down",
    }
}

/// The trigger in the format of the XDG shortcuts specification: uppercase modifier
/// names (`CTRL`, `ALT`, `SHIFT`, `LOGO`) and an XKB key name, joined by `+`.
pub fn portal_trigger(chord: &Chord) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (on, name) in [
        (chord.ctrl, "CTRL"),
        (chord.alt, "ALT"),
        (chord.shift, "SHIFT"),
        (chord.logo, "LOGO"),
    ] {
        if on {
            parts.push(name.to_owned());
        }
    }
    parts.push(match chord.key {
        Key::Char(c) => c.to_string(),
        Key::Function(n) => format!("F{n}"),
        Key::Named(named) => xkb_name(named).to_owned(),
    });
    parts.join("+")
}

fn status_of(shortcut: &Shortcut, registration: Option<String>) -> ShortcutStatus {
    let trigger = shortcut.trigger_description().trim();
    if trigger.is_empty() {
        ShortcutStatus::Unassigned
    } else {
        ShortcutStatus::Active {
            trigger: trigger.to_owned(),
            registration,
        }
    }
}

/// The D-Bus error name behind a failed call, when there is one.
fn dbus_error_name(err: &Error) -> Option<String> {
    match err {
        Error::Zbus(zbus::Error::MethodError(name, ..))
        | Error::Portal(PortalError::ZBus(zbus::Error::MethodError(name, ..))) => {
            Some(name.as_str().to_owned())
        }
        _ => None,
    }
}

/// The service, interface or object the call went to does not exist.
fn is_missing_interface(err: &Error) -> bool {
    matches!(err, Error::PortalNotFound(_))
        || dbus_error_name(err).is_some_and(|name| {
            name.ends_with("UnknownMethod")
                || name.ends_with("UnknownInterface")
                || name.ends_with("UnknownObject")
        })
}

/// A short explanation of a portal error, for the user.
fn explain(err: &Error) -> String {
    if matches!(err, Error::Response(ResponseError::Cancelled)) {
        return "the shortcut request was declined".to_owned();
    }
    if dbus_error_name(err).is_some_and(|name| {
        name.ends_with("ServiceUnknown")
            || name.ends_with("NameHasNoOwner")
            || name.ends_with("NoReply")
    }) {
        return "no desktop portal is running on this session".to_owned();
    }
    if is_missing_interface(err) {
        return no_global_shortcuts();
    }
    err.to_string()
}

/// Same, for the application registry, which older portals do not have.
fn explain_registration(err: &Error) -> String {
    if is_missing_interface(err) {
        "this desktop portal has no application registry".to_owned()
    } else {
        explain(err)
    }
}

fn no_global_shortcuts() -> String {
    "the desktop portal does not offer global shortcuts (the portal for this desktop is \
     missing or too old)"
        .to_owned()
}

/// Start the shortcut task. It reports through `status` and sends
/// [`Command::Toggle`] on `commands` each time the shortcut fires. It ends when the
/// command channel closes or when the shortcut stops working (after reporting it).
///
/// Must be called from inside a Tokio runtime.
pub fn spawn(
    bus: Bus,
    settings: Settings,
    commands: UnboundedSender<Command>,
    status: UnboundedSender<ShortcutStatus>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The portal client runs in its own task so that a panic inside it (the
        // library asserts on a malformed reply) is reported instead of lost.
        let inner = tokio::spawn({
            let status = status.clone();
            async move { run(&bus, &settings, &commands, &status).await }
        });
        let reason = match inner.await {
            Ok(Ok(())) => return,
            Ok(Err(reason)) => reason,
            Err(_) => "the portal client failed unexpectedly".to_owned(),
        };
        let _ = status.send(ShortcutStatus::Unavailable { reason });
    })
}

async fn run(
    bus: &Bus,
    settings: &Settings,
    commands: &UnboundedSender<Command>,
    status: &UnboundedSender<ShortcutStatus>,
) -> Result<(), String> {
    let connection = connect(bus)
        .await
        .map_err(|err| format!("cannot reach the session bus: {err}"))?;

    // An unsandboxed application has no id the portal can see, so it declares one.
    // A refusal is not fatal (older portals have no registry, and a launcher may
    // already have given the process an id) but it is reported along with the result.
    let app_id = AppID::try_from(settings.app_id.as_str())
        .map_err(|_| "the application id is not valid for the desktop portal".to_owned())?;
    let registration = register_host_app_with_connection(connection.clone(), app_id)
        .await
        .err()
        .map(|err| explain_registration(&err));

    let portal = GlobalShortcuts::with_connection(connection)
        .await
        .map_err(|err| explain(&err))?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|err| explain(&err))?;

    // Subscribe before binding so no activation can be missed.
    let mut activated = Box::pin(
        portal
            .receive_activated()
            .await
            .map_err(|err| explain(&err))?,
    );
    let mut changed = Box::pin(
        portal
            .receive_shortcuts_changed()
            .await
            .map_err(|err| explain(&err))?,
    );
    let mut closed = Box::pin(
        session
            .receive_closed()
            .await
            .map_err(|err| explain(&err))?,
    );

    let trigger = portal_trigger(&settings.chord);
    let bound = portal
        .bind_shortcuts(
            &session,
            &[NewShortcut::new(SHORTCUT_ID, DESCRIPTION).preferred_trigger(trigger.as_str())],
            None,
            Default::default(),
        )
        .await
        .map_err(|err| explain(&err))?
        .response()
        .map_err(|err| explain(&err))?;
    let first = bound
        .shortcuts()
        .iter()
        .find(|shortcut| shortcut.id() == SHORTCUT_ID)
        .ok_or_else(|| "the portal did not register the shortcut".to_owned())?;
    if status.send(status_of(first, registration.clone())).is_err() {
        return Ok(());
    }

    let lost = || "the connection to the desktop portal was lost".to_owned();
    loop {
        tokio::select! {
            event = activated.next() => {
                let event = event.ok_or_else(lost)?;
                // The portal addresses these signals to this connection, and this
                // connection owns one session, so the shortcut id is enough.
                if event.shortcut_id() == SHORTCUT_ID && commands.send(Command::Toggle).is_err() {
                    return Ok(());
                }
            }
            event = changed.next() => {
                let event = event.ok_or_else(lost)?;
                if let Some(shortcut) = event.shortcuts().iter().find(|s| s.id() == SHORTCUT_ID)
                    && status.send(status_of(shortcut, registration.clone())).is_err()
                {
                    return Ok(());
                }
            }
            event = closed.next() => {
                event.ok_or_else(lost)?;
                return Err("the desktop portal closed the shortcut session".to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(text: &str) -> String {
        portal_trigger(&text.parse().unwrap())
    }

    #[test]
    fn the_default_shortcut_uses_the_portal_format() {
        assert_eq!(trigger("CTRL+SHIFT+SPACE"), "CTRL+SHIFT+space");
    }

    #[test]
    fn modifiers_are_uppercase_and_super_is_logo() {
        assert_eq!(trigger("super+alt+k"), "ALT+LOGO+k");
        assert_eq!(trigger("ctrl+alt+shift+super+7"), "CTRL+ALT+SHIFT+LOGO+7");
    }

    #[test]
    fn keys_use_xkb_names() {
        assert_eq!(trigger("ctrl+enter"), "CTRL+Return");
        assert_eq!(trigger("ctrl+backspace"), "CTRL+BackSpace");
        assert_eq!(trigger("alt+pgup"), "ALT+Page_Up");
        assert_eq!(trigger("alt+PageDown"), "ALT+Page_Down");
        assert_eq!(trigger("ctrl+esc"), "CTRL+Escape");
        assert_eq!(trigger("F9"), "F9");
        assert_eq!(trigger("CTRL+up"), "CTRL+Up");
    }

    #[test]
    fn letters_are_lowercase() {
        assert_eq!(trigger("CTRL+SHIFT+A"), "CTRL+SHIFT+a");
    }
}
