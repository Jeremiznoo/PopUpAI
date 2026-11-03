//! MCP servers already configured on this computer by other applications, found by reading
//! their configuration files (read-only).
//!
//! Nothing is started here. A server is a program, so each one has to be switched on by the user
//! before it is run. The `env` values of a server often hold API keys: they are kept only to be
//! passed to that server's process, and are never shown, logged or sent to a model.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

#[derive(Clone, PartialEq, Eq)]
pub struct McpServer {
    /// Stable id: `source/name`.
    pub id: String,
    pub name: String,
    /// The application whose configuration it came from.
    pub source: &'static str,
    pub command: String,
    pub args: Vec<String>,
    /// Environment for the server's process. Secrets live here: never printed.
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("id", &self.id)
            .field("command", &self.command)
            .field("env_vars", &self.env.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl McpServer {
    /// What the settings page shows: the command and its arguments, no environment. Arguments
    /// can carry secrets too (`--header "Authorization: Bearer ..."`), so those are hidden.
    pub fn summary(&self) -> String {
        let mut text = self.command.clone();
        let mut previous_is_header = false;
        for arg in &self.args {
            text.push(' ');
            if previous_is_header {
                text.push('\u{2026}');
            } else {
                text.push_str(&hide_secret(arg));
            }
            previous_is_header = matches!(arg.as_str(), "--header" | "-H" | "--headers");
        }
        if text.len() > 90 {
            text.truncate(text.floor_char_boundary(90));
            text.push('\u{2026}');
        }
        text
    }
}

/// An argument that names a credential keeps its name and loses its value.
fn hide_secret(arg: &str) -> String {
    const WORDS: [&str; 8] = [
        "authorization",
        "bearer",
        "token",
        "secret",
        "password",
        "api_key",
        "apikey",
        "api-key",
    ];
    let lowered = arg.to_ascii_lowercase();
    if WORDS.iter().any(|w| lowered.contains(w)) {
        let name = arg.split([':', '=']).next().unwrap_or("");
        return format!("{name}=\u{2026}");
    }
    popupai_security::redact(arg)
}

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn read_json(path: &Path) -> Option<Value> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// A server entry in the usual `{ "command": "...", "args": [...], "env": {...} }` shape.
fn standard_entry(source: &'static str, name: &str, entry: &Value) -> Option<McpServer> {
    // Remote servers (a URL) are not started by this application.
    let command = text(entry.get("command")?)?;
    let args = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(text).collect())
        .unwrap_or_default();
    let env = entry
        .get("env")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), text(v)?)))
                .collect()
        })
        .unwrap_or_default();
    valid_name(name).then(|| McpServer {
        id: format!("{source}/{name}"),
        name: name.to_owned(),
        source,
        command,
        args,
        env,
    })
}

/// opencode writes `"command": ["npx", "-y", "server"]` and `"environment": {...}`.
fn opencode_entry(name: &str, entry: &Value) -> Option<McpServer> {
    if entry.get("type").and_then(Value::as_str) == Some("remote") {
        return None;
    }
    let mut parts: Vec<String> = match entry.get("command")? {
        Value::Array(a) => a.iter().filter_map(text).collect(),
        Value::String(s) => vec![s.clone()],
        _ => return None,
    };
    if parts.is_empty() {
        return None;
    }
    let command = parts.remove(0);
    let env = entry
        .get("environment")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), text(v)?)))
                .collect()
        })
        .unwrap_or_default();
    valid_name(name).then(|| McpServer {
        id: format!("opencode/{name}"),
        name: name.to_owned(),
        source: "opencode",
        command,
        args: parts,
        env,
    })
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn from_map(source: &'static str, map: Option<&Value>, out: &mut Vec<McpServer>) {
    if let Some(servers) = map.and_then(Value::as_object) {
        out.extend(
            servers
                .iter()
                .filter_map(|(name, entry)| standard_entry(source, name, entry)),
        );
    }
}

/// Every local MCP server configured under `home` by a known application.
pub fn detect(home: &Path) -> Vec<McpServer> {
    let mut out = Vec::new();
    if let Some(config) = read_json(&home.join(".claude.json")) {
        from_map("Claude Code", config.get("mcpServers"), &mut out);
    }
    if let Some(config) = read_json(&home.join(".config/Claude/claude_desktop_config.json")) {
        from_map("Claude Desktop", config.get("mcpServers"), &mut out);
    }
    if let Some(config) = read_json(&home.join(".cursor/mcp.json")) {
        from_map("Cursor", config.get("mcpServers"), &mut out);
    }
    if let Some(config) = read_json(&home.join(".config/Code/User/mcp.json")) {
        from_map("VS Code", config.get("servers"), &mut out);
    }
    if let Some(config) = read_json(&home.join(".config/opencode/opencode.json"))
        && let Some(servers) = config.get("mcp").and_then(Value::as_object)
    {
        out.extend(
            servers
                .iter()
                .filter_map(|(name, entry)| opencode_entry(name, entry)),
        );
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Home(std::path::PathBuf);

    impl Home {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("popupai-mcp-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn servers_are_found_in_each_known_configuration() {
        let home = Home::new("known");
        home.write(
            ".config/Claude/claude_desktop_config.json",
            r#"{"mcpServers":{"files":{"command":"npx","args":["-y","@x/files","/home"],"env":{"TOKEN":"s3cret"}}}}"#,
        );
        home.write(
            ".claude.json",
            r#"{"mcpServers":{"git":{"command":"uvx","args":["mcp-git"]}},"other":1}"#,
        );
        home.write(
            ".config/opencode/opencode.json",
            r#"{"mcp":{"docs":{"type":"local","command":["node","server.js"],"environment":{"K":"v"}},"web":{"type":"remote","url":"https://x"}}}"#,
        );
        home.write(
            ".cursor/mcp.json",
            r#"{"mcpServers":{"c":{"command":"c"}}}"#,
        );
        home.write(
            ".config/Code/User/mcp.json",
            r#"{"servers":{"v":{"command":"v","args":["--x"]}}}"#,
        );
        let found = detect(&home.0);
        let ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "Claude Code/git",
                "Claude Desktop/files",
                "Cursor/c",
                "VS Code/v",
                "opencode/docs"
            ]
        );
        let files = found.iter().find(|s| s.name == "files").unwrap();
        assert_eq!((files.command.as_str(), files.args.len()), ("npx", 3));
        let docs = found.iter().find(|s| s.name == "docs").unwrap();
        assert_eq!(
            (docs.command.as_str(), docs.args.as_slice()),
            ("node", ["server.js".to_owned()].as_slice())
        );
    }

    #[test]
    fn remote_servers_broken_files_and_bad_names_are_skipped() {
        let home = Home::new("skip");
        home.write(".claude.json", "{ not json");
        home.write(".cursor/mcp.json", r#"{"mcpServers":{"web":{"url":"https://x"},"bad name!":{"command":"x"},"ok":{"command":"x"}}}"#);
        let found = detect(&home.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "ok");
        assert!(detect(&home.0.join("nowhere")).is_empty());
    }

    #[test]
    fn secrets_in_the_environment_never_reach_the_summary_or_the_debug_output() {
        let home = Home::new("secret");
        home.write(
            ".cursor/mcp.json",
            r#"{"mcpServers":{"s":{"command":"npx","args":["srv"],"env":{"API_KEY":"sk-very-secret-value"}}}}"#,
        );
        let server = detect(&home.0).remove(0);
        assert_eq!(
            server.env.get("API_KEY").map(String::as_str),
            Some("sk-very-secret-value")
        );
        assert_eq!(server.summary(), "npx srv");
        let debug = format!("{server:?}");
        assert!(!debug.contains("sk-very-secret-value"), "{debug}");
        assert!(
            debug.contains("API_KEY"),
            "only the variable names: {debug}"
        );
    }

    #[test]
    fn credentials_in_arguments_are_hidden_in_the_summary() {
        let server = McpServer {
            id: "x/y".into(),
            name: "y".into(),
            source: "x",
            command: "mcp-remote".into(),
            args: vec![
                "http://127.0.0.1:2187/mcp".into(),
                "--header".into(),
                "Authorization: Bearer abcdef0123456789".into(),
                "--api-key=sk-abcdef0123456789".into(),
                "--token".into(),
            ],
            env: BTreeMap::new(),
        };
        let summary = server.summary();
        assert!(
            summary.starts_with("mcp-remote http://127.0.0.1:2187/mcp --header"),
            "{summary}"
        );
        for leaked in ["abcdef0123456789", "Bearer", "sk-"] {
            assert!(!summary.contains(leaked), "{summary}");
        }
    }

    #[test]
    fn a_long_command_is_shortened_in_the_summary() {
        let server = McpServer {
            id: "x/y".into(),
            name: "y".into(),
            source: "x",
            command: "npx".into(),
            args: vec!["a".repeat(200)],
            env: BTreeMap::new(),
        };
        assert!(server.summary().chars().count() <= 92);
    }
}
