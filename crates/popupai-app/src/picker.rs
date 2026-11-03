//! The model list shown by the picker (Ctrl+P), built without Qt so the ordering rules
//! can be tested: recent models first, then favourites, then every provider's models.

use popupai_core::orchestrator::ModelChoice;
use popupai_core::state::{ModelRef, UserState};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The section heading: "Recent", "Favorites" or a provider name.
    pub group: String,
    pub provider: String,
    pub provider_label: String,
    /// The model name. Not called `model`: in a QML delegate that name is the delegate's own
    /// model object, and a role of that name would hide it.
    #[serde(rename = "name")]
    pub model: String,
    pub favorite: bool,
    /// The model the next question would use.
    pub current: bool,
    pub requires_key: bool,
}

pub fn build(choices: &[ModelChoice], state: &UserState, current: Option<&ModelRef>) -> Vec<Entry> {
    let entry = |choice: &ModelChoice, group: &str| {
        let model_ref = ModelRef::new(&choice.provider, &choice.model);
        Entry {
            group: group.to_owned(),
            provider: choice.provider.clone(),
            provider_label: choice.provider_label.clone(),
            model: choice.model.clone(),
            favorite: state.is_favorite(&model_ref),
            current: current == Some(&model_ref),
            requires_key: choice.requires_key,
        }
    };
    let find = |wanted: &ModelRef| {
        choices
            .iter()
            .find(|c| c.provider == wanted.provider && c.model == wanted.model)
    };

    let mut out = Vec::new();
    for wanted in &state.recent {
        if let Some(choice) = find(wanted) {
            out.push(entry(choice, "Recent"));
        }
    }
    for wanted in &state.favorites {
        if let Some(choice) = find(wanted) {
            out.push(entry(choice, "Favorites"));
        }
    }
    for choice in choices {
        out.push(entry(choice, &choice.provider_label));
    }
    out
}

pub fn to_json(entries: &[Entry]) -> String {
    serde_json::to_string(entries).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(provider: &str, label: &str, model: &str, requires_key: bool) -> ModelChoice {
        ModelChoice {
            provider: provider.into(),
            provider_label: label.into(),
            model: model.into(),
            requires_key,
        }
    }

    fn choices() -> Vec<ModelChoice> {
        vec![
            choice("anthropic", "Claude", "claude-haiku-5-5", true),
            choice("ollama", "Ollama", "llama3.2:1b", false),
            choice("ollama", "Ollama", "qwen3:0.6b", false),
        ]
    }

    fn groups(entries: &[Entry]) -> Vec<(&str, &str)> {
        entries
            .iter()
            .map(|e| (e.group.as_str(), e.model.as_str()))
            .collect()
    }

    #[test]
    fn without_history_each_provider_lists_its_models() {
        let entries = build(&choices(), &UserState::default(), None);
        assert_eq!(
            groups(&entries),
            [
                ("Claude", "claude-haiku-5-5"),
                ("Ollama", "llama3.2:1b"),
                ("Ollama", "qwen3:0.6b"),
            ]
        );
    }

    #[test]
    fn recent_then_favourites_then_providers() {
        let mut state = UserState::default();
        state.toggle_favorite(&ModelRef::new("anthropic", "claude-haiku-5-5"));
        state.touch(&ModelRef::new("ollama", "qwen3:0.6b"));
        state.touch(&ModelRef::new("ollama", "llama3.2:1b"));
        let entries = build(
            &choices(),
            &state,
            Some(&ModelRef::new("ollama", "llama3.2:1b")),
        );
        assert_eq!(
            groups(&entries),
            [
                ("Recent", "llama3.2:1b"),
                ("Recent", "qwen3:0.6b"),
                ("Favorites", "claude-haiku-5-5"),
                ("Claude", "claude-haiku-5-5"),
                ("Ollama", "llama3.2:1b"),
                ("Ollama", "qwen3:0.6b"),
            ]
        );
        assert!(entries[0].current && entries[4].current && !entries[1].current);
        assert!(entries[2].favorite && entries[3].favorite);
    }

    #[test]
    fn a_remembered_model_that_is_no_longer_available_is_left_out() {
        let mut state = UserState::default();
        state.touch(&ModelRef::new("ollama", "gone:1b"));
        state.toggle_favorite(&ModelRef::new("ollama", "gone:1b"));
        let entries = build(&choices(), &state, None);
        assert!(
            entries
                .iter()
                .all(|e| e.group != "Recent" && e.group != "Favorites")
        );
    }

    #[test]
    fn the_json_has_the_fields_the_interface_reads() {
        let json = to_json(&build(&choices(), &UserState::default(), None));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let first = &parsed[0];
        for key in [
            "group",
            "provider",
            "provider_label",
            "name",
            "favorite",
            "current",
            "requires_key",
        ] {
            assert!(first.get(key).is_some(), "{key} in {json}");
        }
        assert_eq!(first["requires_key"], true);
    }
}
