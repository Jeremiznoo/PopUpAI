//! What the user picked, kept apart from the settings: the models marked as favourites
//! and the ones used most recently. It lives in its own small file (`state.toml`, next to
//! the configuration, mode 0600) so that choosing a model never rewrites the settings.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{create_private_dir, write_private};

const MAX_RECENT: usize = 5;
const MAX_FAVORITES: usize = 64;
const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UserState {
    pub favorites: Vec<ModelRef>,
    /// Most recent first.
    pub recent: Vec<ModelRef>,
}

impl UserState {
    pub fn is_favorite(&self, model: &ModelRef) -> bool {
        self.favorites.contains(model)
    }

    /// Returns whether the model is now a favourite.
    pub fn toggle_favorite(&mut self, model: &ModelRef) -> bool {
        if let Some(at) = self.favorites.iter().position(|m| m == model) {
            self.favorites.remove(at);
            false
        } else if self.favorites.len() < MAX_FAVORITES {
            self.favorites.push(model.clone());
            true
        } else {
            false
        }
    }

    /// Put a model first in the recent list, without duplicates, keeping the last few.
    pub fn touch(&mut self, model: &ModelRef) {
        self.recent.retain(|m| m != model);
        self.recent.insert(0, model.clone());
        self.recent.truncate(MAX_RECENT);
    }

    /// A missing or unreadable file is an empty state: losing the favourites is a small
    /// loss and must never stop the popup from working.
    pub fn load(path: &Path) -> Self {
        let Ok(meta) = fs::metadata(path) else {
            return Self::default();
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return Self::default();
        }
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text =
            toml::to_string_pretty(self).map_err(|err| std::io::Error::other(err.to_string()))?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        create_private_dir(dir)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("state.toml");
        let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let written = write_private(&tmp, text.as_bytes()).and_then(|()| fs::rename(&tmp, path));
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(provider: &str, model: &str) -> ModelRef {
        ModelRef::new(provider, model)
    }

    #[test]
    fn favourites_toggle_on_and_off() {
        let mut state = UserState::default();
        assert!(state.toggle_favorite(&m("ollama", "llama3.2:1b")));
        assert!(state.is_favorite(&m("ollama", "llama3.2:1b")));
        assert!(!state.toggle_favorite(&m("ollama", "llama3.2:1b")));
        assert!(state.favorites.is_empty());
    }

    #[test]
    fn recent_models_are_most_recent_first_without_duplicates_and_bounded() {
        let mut state = UserState::default();
        for i in 0..8 {
            state.touch(&m("p", &format!("model-{i}")));
        }
        assert_eq!(state.recent.len(), MAX_RECENT);
        assert_eq!(state.recent[0], m("p", "model-7"));
        state.touch(&m("p", "model-5"));
        assert_eq!(state.recent[0], m("p", "model-5"));
        assert_eq!(
            state
                .recent
                .iter()
                .filter(|r| **r == m("p", "model-5"))
                .count(),
            1
        );
    }

    #[test]
    fn it_round_trips_through_a_private_file() {
        let dir = std::env::temp_dir().join(format!("popupai-state-{}", std::process::id()));
        let path = dir.join("state.toml");
        let mut state = UserState::default();
        state.toggle_favorite(&m("anthropic", "claude-haiku-5-5"));
        state.touch(&m("ollama", "llama3.2:1b"));
        state.save(&path).unwrap();
        assert_eq!(UserState::load(&path), state);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_broken_file_is_an_empty_state() {
        let dir = std::env::temp_dir().join(format!("popupai-state-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            UserState::load(&dir.join("absent.toml")),
            UserState::default()
        );
        let broken = dir.join("broken.toml");
        fs::write(&broken, "favorites = [[[").unwrap();
        assert_eq!(UserState::load(&broken), UserState::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
