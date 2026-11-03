//! Which MCP servers exist on this computer, and which of them the user switched on.

use popupai_core::mcp::{McpServer, detect as detect_in};
use popupai_core::state::UserState;

/// The servers configured by other applications in the current user's home.
pub fn detect() -> Vec<McpServer> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
        .map_or_else(Vec::new, |home| detect_in(&home))
}

/// Only the servers the user switched on, which are the only ones ever started.
pub fn enabled(found: &[McpServer], state: &UserState) -> Vec<McpServer> {
    found
        .iter()
        .filter(|server| state.mcp_enabled.contains(&server.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn server(id: &str) -> McpServer {
        McpServer {
            id: id.into(),
            name: id.into(),
            source: "test",
            command: "x".into(),
            args: vec![],
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn only_switched_on_servers_are_enabled_and_a_stale_choice_is_ignored() {
        let mut state = UserState::default();
        state.toggle_mcp("a");
        state.toggle_mcp("gone");
        let found = [server("a"), server("b")];
        let ids: Vec<String> = enabled(&found, &state).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["a"]);
        assert!(
            enabled(&found, &UserState::default()).is_empty(),
            "nothing runs by default"
        );
    }
}
