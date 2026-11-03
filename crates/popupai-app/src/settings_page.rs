//! The rows of the settings page, built without Qt so their wording and order can be tested.

use popupai_core::mcp::McpServer;
use popupai_core::orchestrator::ConnectionStatus;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// "provider", "autostart" or "shortcut": what Enter does on the row.
    pub kind: &'static str,
    pub group: &'static str,
    /// The provider id for a provider row, empty otherwise.
    pub id: String,
    pub title: String,
    pub detail: String,
    /// Whether the credential can be removed (Delete).
    pub removable: bool,
}

/// `autostart` is `None` when the desktop has no autostart folder to use. `mcp` lists the
/// servers found on the computer, with whether the user switched each one on.
pub fn build(
    status: &[ConnectionStatus],
    mcp: &[(McpServer, bool)],
    autostart: Option<bool>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = status
        .iter()
        .map(|s| {
            let detail = match (s.requires_key, s.connected, s.signs_in) {
                (false, ..) => "Local, no sign-in needed",
                (true, true, true) => "Signed in",
                (true, true, false) => "API key stored",
                (true, false, true) => "Not connected \u{b7} press Enter to sign in",
                (true, false, false) => "Not connected \u{b7} press Enter to add an API key",
            };
            Row {
                kind: "provider",
                group: "Connections",
                id: s.id.clone(),
                title: s.label.clone(),
                detail: detail.to_owned(),
                removable: s.requires_key && s.connected,
            }
        })
        .collect();
    rows.extend(mcp.iter().map(|(server, enabled)| Row {
        kind: "mcp",
        group: "MCP servers found on this computer",
        id: server.id.clone(),
        title: format!("{} \u{b7} {}", server.name, server.source),
        detail: format!(
            "{} \u{b7} {}",
            if *enabled {
                "On"
            } else {
                "Off, press Enter to turn on"
            },
            server.summary()
        ),
        removable: false,
    }));
    rows.push(Row {
        kind: "autostart",
        group: "General",
        id: String::new(),
        title: "Start with the session".to_owned(),
        detail: match autostart {
            Some(true) => "On \u{b7} press Enter to turn off",
            Some(false) => "Off \u{b7} press Enter to turn on",
            None => "Not available on this desktop",
        }
        .to_owned(),
        removable: false,
    });
    rows.push(Row {
        kind: "shortcut",
        group: "General",
        id: String::new(),
        title: "Global shortcut".to_owned(),
        detail: "Press Enter to set it in the desktop's shortcut settings".to_owned(),
        removable: false,
    });
    rows
}

pub fn to_json(rows: &[Row]) -> String {
    serde_json::to_string(rows).unwrap_or_else(|_| "[]".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: &str, requires_key: bool, signs_in: bool, connected: bool) -> ConnectionStatus {
        ConnectionStatus {
            id: id.into(),
            label: id.to_uppercase(),
            requires_key,
            signs_in,
            connected,
        }
    }

    #[test]
    fn each_provider_says_how_to_connect_or_that_it_is() {
        let rows = build(
            &[
                status("anthropic", true, false, false),
                status("chatgpt", true, true, false),
                status("gemini", true, false, true),
                status("chatgpt2", true, true, true),
                status("ollama", false, false, true),
            ],
            &[],
            Some(false),
        );
        let details: Vec<&str> = rows.iter().take(5).map(|r| r.detail.as_str()).collect();
        assert_eq!(
            details,
            [
                "Not connected \u{b7} press Enter to add an API key",
                "Not connected \u{b7} press Enter to sign in",
                "API key stored",
                "Signed in",
                "Local, no sign-in needed",
            ]
        );
    }

    #[test]
    fn only_a_stored_credential_can_be_removed() {
        let rows = build(
            &[
                status("a", true, false, true),
                status("b", true, false, false),
                status("o", false, false, true),
            ],
            &[],
            None,
        );
        let removable: Vec<bool> = rows.iter().take(3).map(|r| r.removable).collect();
        assert_eq!(removable, [true, false, false]);
    }

    #[test]
    fn the_general_rows_follow_the_connections() {
        let rows = build(&[status("a", true, false, true)], &[], Some(true));
        let kinds: Vec<&str> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["provider", "autostart", "shortcut"]);
        assert!(rows[1].detail.starts_with("On"));
        assert!(build(&[], &[], None)[0].detail.contains("Not available"));
    }

    #[test]
    fn the_json_carries_what_the_interface_reads() {
        let json = to_json(&build(&[status("a", true, false, true)], &[], Some(false)));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in ["kind", "group", "id", "title", "detail", "removable"] {
            assert!(parsed[0].get(key).is_some(), "{key} in {json}");
        }
    }

    #[test]
    fn found_mcp_servers_are_listed_off_by_default_without_their_environment() {
        let server = McpServer {
            id: "Cursor/files".into(),
            name: "files".into(),
            source: "Cursor",
            command: "npx".into(),
            args: vec!["-y".into(), "srv".into()],
            env: [("API_KEY".to_owned(), "sk-secret".to_owned())].into(),
        };
        let rows = build(&[], &[(server.clone(), false), (server, true)], None);
        assert_eq!(rows[0].kind, "mcp");
        assert!(rows[0].detail.starts_with("Off"), "{}", rows[0].detail);
        assert!(rows[1].detail.starts_with("On"), "{}", rows[1].detail);
        let json = to_json(&rows);
        assert!(
            !json.contains("sk-secret") && !json.contains("API_KEY"),
            "{json}"
        );
        assert!(rows[0].detail.contains("npx -y srv"));
    }
}
