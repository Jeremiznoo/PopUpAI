//! The notification-area icon, as a StatusNotifierItem (KDE Plasma natively, GNOME with
//! the AppIndicator extension).
//!
//! A click toggles the popup, and the menu offers the same plus Quit. The tray only sends
//! [`Command`]s, exactly as the shortcut and `popupai --toggle` do.

use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, Tray, TrayMethods};
use tokio::sync::mpsc::UnboundedSender;

use crate::ipc::{BUS_NAME, Command};

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("no system tray is available on this desktop: {0}")]
    Unavailable(String),
}

struct PopupTray {
    commands: UnboundedSender<Command>,
}

impl PopupTray {
    fn send(&self, command: Command) {
        // The application may already be shutting down.
        let _ = self.commands.send(command);
    }
}

impl Tray for PopupTray {
    fn id(&self) -> String {
        BUS_NAME.to_owned()
    }

    fn title(&self) -> String {
        "PopUpAI".to_owned()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    /// The icon is drawn from embedded pixmaps rather than looked up by name, so it shows
    /// the same whether or not the icon theme entry has been installed.
    fn icon_pixmap(&self) -> Vec<Icon> {
        const SIZES: [(i32, &[u8]); 4] = [
            (22, include_bytes!("../assets/tray-22.argb")),
            (32, include_bytes!("../assets/tray-32.argb")),
            (48, include_bytes!("../assets/tray-48.argb")),
            (64, include_bytes!("../assets/tray-64.argb")),
        ];
        SIZES
            .iter()
            .map(|(side, data)| Icon {
                width: *side,
                height: *side,
                data: data.to_vec(),
            })
            .collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Ask a question".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Show)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit PopUpAI".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| tray.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Keeps the tray icon alive. Dropping it removes the icon.
pub struct TrayHandle {
    _handle: ksni::Handle<PopupTray>,
}

/// Show the icon. Fails when no StatusNotifier host is running.
pub async fn spawn(commands: UnboundedSender<Command>) -> Result<TrayHandle, TrayError> {
    let handle = PopupTray { commands }
        .spawn()
        .await
        .map_err(|err| TrayError::Unavailable(err.to_string()))?;
    Ok(TrayHandle { _handle: handle })
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;

    fn labels_and_commands() -> Vec<(String, Command)> {
        let (tx, mut rx) = unbounded_channel();
        let mut tray = PopupTray { commands: tx };
        let mut seen = Vec::new();
        for item in tray.menu() {
            if let MenuItem::Standard(item) = item {
                (item.activate)(&mut tray);
                seen.push((item.label, rx.try_recv().expect("the item sends a command")));
            }
        }
        seen
    }

    #[test]
    fn the_menu_offers_ask_and_quit() {
        assert_eq!(
            labels_and_commands(),
            [
                ("Ask a question".to_owned(), Command::Show),
                ("Quit PopUpAI".to_owned(), Command::Quit),
            ]
        );
    }

    #[test]
    fn a_click_toggles_the_popup() {
        let (tx, mut rx) = unbounded_channel();
        let mut tray = PopupTray { commands: tx };
        tray.activate(0, 0);
        assert_eq!(rx.try_recv().ok(), Some(Command::Toggle));
    }

    #[test]
    fn a_closed_channel_does_not_panic() {
        let (tx, rx) = unbounded_channel();
        drop(rx);
        let mut tray = PopupTray { commands: tx };
        tray.activate(0, 0);
    }

    #[test]
    fn the_tray_icon_has_complete_pixmaps_in_several_sizes() {
        let (tx, _rx) = unbounded_channel();
        let icons = PopupTray { commands: tx }.icon_pixmap();
        let sides: Vec<i32> = icons.iter().map(|i| i.width).collect();
        assert_eq!(sides, [22, 32, 48, 64]);
        for icon in &icons {
            assert_eq!(icon.width, icon.height);
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
            // ARGB: the corner of the square is transparent, the middle is not.
            assert_eq!(icon.data[0], 0, "the corner is transparent");
            let middle = ((icon.height / 2 * icon.width + icon.width / 2) * 4) as usize;
            assert_eq!(icon.data[middle], 255, "the middle is opaque");
        }
    }

    #[test]
    fn the_icon_is_identified_by_the_application_id() {
        let (tx, _rx) = unbounded_channel();
        assert_eq!(
            PopupTray { commands: tx }.id(),
            "io.github.jeremiznoo.PopUpAI"
        );
    }
}
