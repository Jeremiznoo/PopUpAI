//! Binding the shortcut in the desktop itself, so that it starts PopUpAI when it is not running.
//!
//! A portal shortcut only exists while the application that registered it runs. To open the popup
//! from nothing, the desktop must own the shortcut and run `popupai --toggle`. Each desktop does
//! that its own way: this module works out what to run for the ones with a command-line interface
//! (KDE Plasma, GNOME, XFCE) and gives a line to paste for the others. Planning is pure, so it is
//! tested; running the commands is left to the caller, which asks the user first.

use popupai_core::shortcut::Chord;

use crate::shortcut::portal_trigger;

/// The desktop environments told apart. `Other` carries the name found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Desktop {
    Kde,
    Gnome,
    Xfce,
    Sway,
    I3,
    Hyprland,
    Other(String),
}

/// What `XDG_CURRENT_DESKTOP` (a colon-separated list, such as `ubuntu:GNOME`) says.
pub fn detect(xdg_current_desktop: &str) -> Desktop {
    let names: Vec<String> = xdg_current_desktop
        .split(':')
        .map(|n| n.trim().to_ascii_lowercase())
        .collect();
    let has = |wanted: &[&str]| names.iter().any(|n| wanted.contains(&n.as_str()));
    if has(&["kde"]) {
        Desktop::Kde
    } else if has(&["gnome", "ubuntu", "pop", "zorin", "unity", "budgie"]) {
        Desktop::Gnome
    } else if has(&["xfce"]) {
        Desktop::Xfce
    } else if has(&["sway"]) {
        Desktop::Sway
    } else if has(&["i3"]) {
        Desktop::I3
    } else if has(&["hyprland"]) {
        Desktop::Hyprland
    } else {
        Desktop::Other(xdg_current_desktop.trim().to_owned())
    }
}

/// What to do, in order: programs to run (never through a shell), and a text for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub desktop: Desktop,
    /// Each entry is a program and its arguments.
    pub commands: Vec<Vec<String>>,
    /// Shown to the user: what is done, or what to paste where.
    pub note: String,
}

const GNOME_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/popupai/";

fn modifiers(chord: &Chord, names: [&str; 4]) -> String {
    let mut out = String::new();
    for (on, name) in [
        (chord.ctrl, names[0]),
        (chord.alt, names[1]),
        (chord.shift, names[2]),
        (chord.logo, names[3]),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out
}

/// The key part of a trigger in XKB spelling (`space`, `Return`, `a`, `F9`...).
fn key_name(chord: &Chord) -> String {
    portal_trigger(chord)
        .rsplit('+')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// `gsettings` writes a list of strings as `['a', 'b']`.
fn gvariant_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
    format!("[{}]", inner.join(", "))
}

/// Parse what `gsettings get ... custom-keybindings` prints, such as `['/a/', '/b/']` or `@as []`.
pub fn parse_gvariant_list(text: &str) -> Vec<String> {
    text.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `exe` is the full path of the program; `existing_gnome` the custom keybindings GNOME already has.
pub fn plan(desktop: &Desktop, chord: &Chord, exe: &str, existing_gnome: &[String]) -> Plan {
    let command = format!("{exe} --toggle");
    match desktop {
        Desktop::Gnome => {
            let mut list = existing_gnome.to_vec();
            if !list.iter().any(|p| p == GNOME_PATH) {
                list.push(GNOME_PATH.to_owned());
            }
            let schema = format!("{GNOME_KEYS}.custom-keybinding:{GNOME_PATH}");
            let binding = format!(
                "{}{}",
                modifiers(chord, ["<Control>", "<Alt>", "<Shift>", "<Super>"]),
                key_name(chord)
            );
            let set = |schema: &str, key: &str, value: String| {
                vec![
                    "gsettings".to_owned(),
                    "set".to_owned(),
                    schema.to_owned(),
                    key.to_owned(),
                    value,
                ]
            };
            Plan {
                desktop: desktop.clone(),
                commands: vec![
                    set(GNOME_KEYS, "custom-keybindings", gvariant_list(&list)),
                    set(&schema, "name", "PopUpAI".to_owned()),
                    set(&schema, "command", command.clone()),
                    set(&schema, "binding", binding.clone()),
                ],
                note: format!(
                    "Adds a custom keyboard shortcut in GNOME: {binding} runs {command}."
                ),
            }
        }
        Desktop::Xfce => {
            let binding = format!(
                "{}{}",
                modifiers(chord, ["<Primary>", "<Alt>", "<Shift>", "<Super>"]),
                key_name(chord)
            );
            Plan {
                desktop: desktop.clone(),
                commands: vec![vec![
                    "xfconf-query".to_owned(),
                    "-c".to_owned(),
                    "xfce4-keyboard-shortcuts".to_owned(),
                    "-p".to_owned(),
                    format!("/commands/custom/{binding}"),
                    "-n".to_owned(),
                    "-t".to_owned(),
                    "string".to_owned(),
                    "-s".to_owned(),
                    command.clone(),
                ]],
                note: format!("Adds a custom shortcut in XFCE: {binding} runs {command}."),
            }
        }
        Desktop::Kde => {
            let key = chord
                .to_string()
                .split('+')
                .map(title)
                .collect::<Vec<_>>()
                .join("+");
            Plan {
                desktop: desktop.clone(),
                commands: vec![vec![
                    "kwriteconfig6".to_owned(),
                    "--file".to_owned(),
                    "kglobalshortcutsrc".to_owned(),
                    "--group".to_owned(),
                    "services".to_owned(),
                    "--group".to_owned(),
                    "io.github.jeremiznoo.PopUpAI.desktop".to_owned(),
                    "--key".to_owned(),
                    "_launch".to_owned(),
                    key.clone(),
                ]],
                note: format!(
                    "Binds {key} to the PopUpAI launcher (which runs {command}) in KDE's shortcuts. The launcher \
                     must be installed (tools/install-local.sh or the package). Plasma reads it at the next login, or \
                     after `kquitapp6 kglobalaccel && kglobalaccel6 &`."
                ),
            }
        }
        Desktop::Sway | Desktop::I3 => {
            let line = format!(
                "bindsym {}{} exec {command}",
                modifiers(chord, ["ctrl+", "alt+", "shift+", "Mod4+"]),
                key_name(chord)
            );
            Plan {
                desktop: desktop.clone(),
                commands: vec![],
                note: format!(
                    "Add this line to your {} config, then reload it:\n\n    {line}",
                    if *desktop == Desktop::Sway {
                        "sway"
                    } else {
                        "i3"
                    }
                ),
            }
        }
        Desktop::Hyprland => {
            let mods = modifiers(chord, ["CTRL ", "ALT ", "SHIFT ", "SUPER "])
                .trim()
                .replace("  ", " ");
            Plan {
                desktop: desktop.clone(),
                commands: vec![],
                note: format!(
                    "Add this line to ~/.config/hypr/hyprland.conf, then reload it:\n\n    bind = {mods}, {}, exec, {command}",
                    key_name(chord)
                ),
            }
        }
        Desktop::Other(name) => Plan {
            desktop: desktop.clone(),
            commands: vec![],
            note: format!(
                "No automatic setup for this desktop{}. In its keyboard settings, add a custom shortcut for {} \
                 that runs:\n\n    {command}",
                if name.is_empty() {
                    String::new()
                } else {
                    format!(" ({name})")
                },
                chord
            ),
        },
    }
}

fn title(part: &str) -> String {
    let lower = part.to_ascii_lowercase();
    let mut chars = lower.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord() -> Chord {
        "CTRL+SHIFT+SPACE".parse().unwrap()
    }

    #[test]
    fn the_desktop_is_recognised_from_the_session_variable() {
        assert_eq!(detect("KDE"), Desktop::Kde);
        assert_eq!(detect("ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(detect("pop:GNOME"), Desktop::Gnome);
        assert_eq!(detect("XFCE"), Desktop::Xfce);
        assert_eq!(detect("sway"), Desktop::Sway);
        assert_eq!(detect("Hyprland"), Desktop::Hyprland);
        assert_eq!(detect("X-Cinnamon"), Desktop::Other("X-Cinnamon".into()));
        assert_eq!(detect(""), Desktop::Other(String::new()));
    }

    #[test]
    fn gnome_gets_a_custom_keybinding_added_to_the_existing_ones() {
        let existing = vec![
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/".to_owned(),
        ];
        let plan = plan(&Desktop::Gnome, &chord(), "/usr/bin/popupai", &existing);
        let all: Vec<String> = plan.commands.iter().map(|c| c.join(" ")).collect();
        assert!(all[0].contains("custom-keybindings ['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/', '/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/popupai/']"), "{}", all[0]);
        assert!(
            all[2].ends_with("command /usr/bin/popupai --toggle"),
            "{}",
            all[2]
        );
        assert!(
            all[3].ends_with("binding <Control><Shift>space"),
            "{}",
            all[3]
        );
        // Running it twice does not add the path twice.
        let again = super::plan(
            &Desktop::Gnome,
            &chord(),
            "/usr/bin/popupai",
            &parse_gvariant_list(&format!("['{GNOME_PATH}']")),
        );
        assert_eq!(
            again.commands[0]
                .last()
                .unwrap()
                .matches("popupai/")
                .count(),
            1
        );
    }

    #[test]
    fn xfce_gets_one_custom_command() {
        let plan = plan(&Desktop::Xfce, &chord(), "/usr/bin/popupai", &[]);
        assert_eq!(plan.commands.len(), 1);
        let args = &plan.commands[0];
        assert_eq!(args[0], "xfconf-query");
        assert!(
            args.contains(&"/commands/custom/<Primary><Shift>space".to_owned()),
            "{args:?}"
        );
        assert_eq!(args.last().unwrap(), "/usr/bin/popupai --toggle");
    }

    #[test]
    fn kde_binds_the_launcher_through_kglobalshortcutsrc() {
        let plan = plan(&Desktop::Kde, &chord(), "/usr/bin/popupai", &[]);
        let args = &plan.commands[0];
        assert_eq!(args[0], "kwriteconfig6");
        assert!(args.contains(&"io.github.jeremiznoo.PopUpAI.desktop".to_owned()));
        assert_eq!(args.last().unwrap(), "Ctrl+Shift+Space");
        assert!(plan.note.contains("next login"), "{}", plan.note);
    }

    #[test]
    fn tiling_desktops_and_unknown_ones_get_a_line_to_paste_and_run_nothing() {
        let sway = plan(&Desktop::Sway, &chord(), "popupai", &[]);
        assert!(sway.commands.is_empty());
        assert!(
            sway.note
                .contains("bindsym ctrl+shift+space exec popupai --toggle"),
            "{}",
            sway.note
        );
        let hypr = plan(&Desktop::Hyprland, &chord(), "popupai", &[]);
        assert!(
            hypr.note
                .contains("bind = CTRL SHIFT, space, exec, popupai --toggle"),
            "{}",
            hypr.note
        );
        let other = plan(
            &Desktop::Other("X-Cinnamon".into()),
            &chord(),
            "popupai",
            &[],
        );
        assert!(
            other.commands.is_empty() && other.note.contains("popupai --toggle"),
            "{}",
            other.note
        );
    }

    #[test]
    fn a_gsettings_list_is_read_back() {
        assert_eq!(parse_gvariant_list("['/a/', '/b/']"), ["/a/", "/b/"]);
        assert!(parse_gvariant_list("@as []").is_empty());
        assert!(parse_gvariant_list("[]").is_empty());
    }
}
