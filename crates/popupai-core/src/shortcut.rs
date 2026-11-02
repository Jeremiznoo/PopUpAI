//! Global shortcut descriptions, independent of any operating system.
//!
//! The configuration stores a shortcut as text such as `CTRL+SHIFT+SPACE`. This module
//! parses and validates it once; each platform then converts the result to its own
//! format (the XDG portal, `RegisterHotKey`, and so on).

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

impl NamedKey {
    const ALL: [(NamedKey, &'static str, &'static [&'static str]); 14] = [
        (Self::Space, "SPACE", &[]),
        (Self::Enter, "ENTER", &["RETURN"]),
        (Self::Tab, "TAB", &[]),
        (Self::Escape, "ESCAPE", &["ESC"]),
        (Self::Backspace, "BACKSPACE", &[]),
        (Self::Delete, "DELETE", &["DEL"]),
        (Self::Up, "UP", &[]),
        (Self::Down, "DOWN", &[]),
        (Self::Left, "LEFT", &[]),
        (Self::Right, "RIGHT", &[]),
        (Self::Home, "HOME", &[]),
        (Self::End, "END", &[]),
        (Self::PageUp, "PAGEUP", &["PGUP"]),
        (Self::PageDown, "PAGEDOWN", &["PGDN"]),
    ];

    fn canonical(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(key, ..)| *key == self)
            .map_or("", |(_, name, _)| name)
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(_, canonical, aliases)| *canonical == name || aliases.contains(&name))
            .map(|(key, ..)| *key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter or a digit, always lowercase.
    Char(char),
    /// `F1` to `F24`.
    Function(u8),
    Named(NamedKey),
}

/// A modifier combination plus one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Super / Windows / Meta key.
    pub logo: bool,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChordError {
    #[error("the shortcut is empty")]
    Empty,
    #[error("`{0}` is not a key or modifier PopUpAI knows")]
    Unknown(String),
    #[error("the {0} modifier is given twice")]
    Duplicate(&'static str),
    #[error("the shortcut must end with a key, not only modifiers")]
    MissingKey,
    #[error("only one key may follow the modifiers")]
    SeveralKeys,
    #[error(
        "a shortcut with no modifier would capture that key in every application; add Ctrl, Alt, Shift or Super (function keys are the exception)"
    )]
    NeedsModifier,
}

fn modifier_name(token: &str) -> Option<&'static str> {
    match token {
        "CTRL" | "CONTROL" => Some("CTRL"),
        "ALT" => Some("ALT"),
        "SHIFT" => Some("SHIFT"),
        "SUPER" | "META" | "LOGO" | "WIN" => Some("SUPER"),
        _ => None,
    }
}

fn parse_key(token: &str) -> Option<Key> {
    let mut chars = token.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(Key::Char(c.to_ascii_lowercase()));
    }
    if let Some(number) = token.strip_prefix('F')
        && let Ok(n) = number.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return Some(Key::Function(n));
    }
    NamedKey::from_name(token).map(Key::Named)
}

impl FromStr for Chord {
    type Err = ChordError;

    fn from_str(text: &str) -> Result<Self, ChordError> {
        if text.trim().is_empty() {
            return Err(ChordError::Empty);
        }
        let (mut ctrl, mut alt, mut shift, mut logo) = (false, false, false, false);
        let mut key = None;
        for raw in text.split('+') {
            let token = raw.trim().to_ascii_uppercase();
            if let Some(name) = modifier_name(&token) {
                let slot = match name {
                    "CTRL" => &mut ctrl,
                    "ALT" => &mut alt,
                    "SHIFT" => &mut shift,
                    _ => &mut logo,
                };
                if std::mem::replace(slot, true) {
                    return Err(ChordError::Duplicate(name));
                }
            } else if let Some(parsed) = parse_key(&token) {
                if key.replace(parsed).is_some() {
                    return Err(ChordError::SeveralKeys);
                }
            } else {
                return Err(ChordError::Unknown(raw.trim().to_owned()));
            }
        }
        let key = key.ok_or(ChordError::MissingKey)?;
        if !(ctrl || alt || shift || logo) && !matches!(key, Key::Function(_)) {
            return Err(ChordError::NeedsModifier);
        }
        Ok(Self {
            ctrl,
            alt,
            shift,
            logo,
            key,
        })
    }
}

/// The canonical form, which parses back to the same chord: `CTRL+SHIFT+SPACE`.
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "CTRL"),
            (self.alt, "ALT"),
            (self.shift, "SHIFT"),
            (self.logo, "SUPER"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        match self.key {
            Key::Char(c) => write!(f, "{}", c.to_ascii_uppercase()),
            Key::Function(n) => write!(f, "F{n}"),
            Key::Named(named) => f.write_str(named.canonical()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        text.parse().unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    #[test]
    fn the_default_shortcut_parses() {
        let c = chord("CTRL+SHIFT+SPACE");
        assert!(c.ctrl && c.shift && !c.alt && !c.logo);
        assert_eq!(c.key, Key::Named(NamedKey::Space));
    }

    #[test]
    fn parsing_ignores_case_and_spaces_and_accepts_aliases() {
        assert_eq!(chord("ctrl + shift + space"), chord("CTRL+SHIFT+SPACE"));
        assert_eq!(chord("Control+Alt+Return"), chord("CTRL+ALT+ENTER"));
        assert_eq!(chord("meta+k"), chord("SUPER+K"));
        assert_eq!(chord("Win+Esc"), chord("SUPER+ESCAPE"));
    }

    #[test]
    fn letters_digits_function_and_named_keys_are_understood() {
        assert_eq!(chord("CTRL+a").key, Key::Char('a'));
        assert_eq!(chord("ALT+7").key, Key::Char('7'));
        assert_eq!(
            chord("F12").key,
            Key::Function(12),
            "a function key needs no modifier"
        );
        assert_eq!(chord("CTRL+F24").key, Key::Function(24));
        assert_eq!(chord("CTRL+PgDn").key, Key::Named(NamedKey::PageDown));
    }

    #[test]
    fn the_canonical_form_round_trips() {
        for text in [
            "CTRL+SHIFT+SPACE",
            "CTRL+ALT+SHIFT+SUPER+K",
            "ALT+F4",
            "F9",
            "SUPER+PAGEUP",
            "CTRL+7",
        ] {
            let c = chord(text);
            assert_eq!(c.to_string(), text);
            assert_eq!(chord(&c.to_string()), c);
        }
        assert_eq!(
            chord("shift+ctrl+space").to_string(),
            "CTRL+SHIFT+SPACE",
            "modifiers are ordered"
        );
    }

    #[test]
    fn invalid_shortcuts_are_refused_with_a_reason() {
        assert_eq!("".parse::<Chord>(), Err(ChordError::Empty));
        assert_eq!("   ".parse::<Chord>(), Err(ChordError::Empty));
        assert_eq!("CTRL+SHIFT".parse::<Chord>(), Err(ChordError::MissingKey));
        assert_eq!("CTRL+A+B".parse::<Chord>(), Err(ChordError::SeveralKeys));
        assert_eq!(
            "CTRL+CTRL+A".parse::<Chord>(),
            Err(ChordError::Duplicate("CTRL"))
        );
        assert_eq!("A".parse::<Chord>(), Err(ChordError::NeedsModifier));
        assert_eq!("SPACE".parse::<Chord>(), Err(ChordError::NeedsModifier));
        assert_eq!(
            "CTRL+Hyper".parse::<Chord>(),
            Err(ChordError::Unknown("Hyper".into()))
        );
        assert_eq!(
            "CTRL++A".parse::<Chord>(),
            Err(ChordError::Unknown(String::new()))
        );
        assert_eq!(
            "F25".parse::<Chord>(),
            Err(ChordError::Unknown("F25".into()))
        );
    }

    #[test]
    fn error_messages_are_readable_for_a_settings_screen() {
        let message = "A".parse::<Chord>().unwrap_err().to_string();
        assert!(
            message.contains("add Ctrl, Alt, Shift or Super"),
            "{message}"
        );
    }
}
