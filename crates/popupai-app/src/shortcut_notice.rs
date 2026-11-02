//! How the state of the global shortcut is told to the user.

use popupai_platform::shortcut::ShortcutStatus;

const MANUAL_ROUTE: &str =
    "Bind the command `popupai --toggle` in your system keyboard settings to open the popup.";

/// The message to show in the popup, or an empty string when everything is fine.
pub fn notice(status: &ShortcutStatus) -> String {
    match status {
        ShortcutStatus::Active { .. } => String::new(),
        ShortcutStatus::Unassigned => format!(
            "The global shortcut has no key assigned. Set one in the system shortcut settings. {MANUAL_ROUTE}"
        ),
        ShortcutStatus::Unavailable { reason } => {
            format!("The global shortcut is unavailable: {reason}. {MANUAL_ROUTE}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_working_shortcut_says_nothing() {
        let status = ShortcutStatus::Active {
            trigger: "Ctrl+Shift+Space".into(),
            registration: None,
        };
        assert_eq!(notice(&status), "");
    }

    #[test]
    fn a_failure_gives_the_reason_and_the_manual_route() {
        let text = notice(&ShortcutStatus::Unavailable {
            reason: "no desktop portal is running on this session".into(),
        });
        assert!(text.contains("no desktop portal is running"), "{text}");
        assert!(text.contains("popupai --toggle"), "{text}");
    }

    #[test]
    fn an_unassigned_shortcut_points_to_the_settings() {
        let text = notice(&ShortcutStatus::Unassigned);
        assert!(text.contains("no key assigned"), "{text}");
        assert!(text.contains("popupai --toggle"), "{text}");
    }
}
