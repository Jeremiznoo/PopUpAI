//! A client for MCP servers over stdio (JSON-RPC, one message per line).
//!
//! A server is a program the user switched on: it is started here with the environment its own
//! configuration gave it, asked for its tools, and called one request at a time. Dropping the
//! connection kills the process.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::mcp::McpServer;

const START_TIMEOUT: Duration = Duration::from_secs(20);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

struct Pipes {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct McpConnection {
    _child: Child,
    pipes: Mutex<Pipes>,
    next_id: AtomicU64,
    pub tools: Vec<McpTool>,
}

impl Pipes {
    async fn send(&mut self, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("could not write to the server: {e}"))?;
        self.stdin.flush().await.map_err(|e| e.to_string())
    }

    /// The result of request `id`; anything else the server says in between is skipped.
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let mut line = String::new();
            let read = (&mut self.stdout)
                .take(MAX_LINE_BYTES as u64)
                .read_line(&mut line)
                .await
                .map_err(|e| format!("could not read from the server: {e}"))?;
            if read == 0 {
                return Err("the server closed the connection".to_owned());
            }
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue; // not JSON: some servers print banners
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("error");
                return Err(format!("the server answered with an error: {text}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl McpConnection {
    /// Start the server and read its tools.
    pub async fn start(server: &McpServer) -> Result<Self, String> {
        let mut child = Command::new(&server.command)
            .args(&server.args)
            .envs(&server.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("could not start `{}`: {e}", server.command))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("could not reach the server's input and output".to_owned());
        };
        let mut pipes = Pipes {
            stdin,
            stdout: BufReader::new(stdout),
        };
        let tools = tokio::time::timeout(START_TIMEOUT, async {
            pipes
                .request(
                    1,
                    "initialize",
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "popupai", "version": env!("CARGO_PKG_VERSION")},
                    }),
                )
                .await?;
            pipes
                .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .await?;
            let listed = pipes.request(2, "tools/list", json!({})).await?;
            Ok::<_, String>(parse_tools(&listed))
        })
        .await
        .map_err(|_| {
            format!(
                "`{}` did not start in {} seconds",
                server.command,
                START_TIMEOUT.as_secs()
            )
        })??;
        Ok(Self {
            _child: child,
            pipes: Mutex::new(pipes),
            next_id: AtomicU64::new(3),
            tools,
        })
    }

    /// Call a tool and return its text. A tool error comes back as text for the model.
    pub async fn call(&self, tool: &str, arguments: Value) -> String {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut pipes = self.pipes.lock().await;
        let outcome = tokio::time::timeout(
            CALL_TIMEOUT,
            pipes.request(
                id,
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
            ),
        )
        .await;
        match outcome {
            Err(_) => format!(
                "the tool did not answer in {} seconds",
                CALL_TIMEOUT.as_secs()
            ),
            Ok(Err(error)) => error,
            Ok(Ok(result)) => result_text(&result),
        }
    }
}

fn parse_tools(listed: &Value) -> Vec<McpTool> {
    listed
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            Some(McpTool {
                name: tool.get("name")?.as_str()?.to_owned(),
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            })
        })
        .collect()
}

fn result_text(result: &Value) -> String {
    let mut text: String = result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        text = format!("the tool reported an error: {text}");
    }
    if text.len() > MAX_RESULT_BYTES {
        text.truncate(text.floor_char_boundary(MAX_RESULT_BYTES));
        text.push_str("\n[output cut]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_read_from_a_listing_and_incomplete_ones_are_skipped() {
        let tools = parse_tools(&json!({"tools": [
            {"name": "a", "description": "d", "inputSchema": {"type": "object", "properties": {}}},
            {"name": "b"},
            {"description": "no name"},
        ]}));
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[1].schema, json!({"type": "object"}));
    }

    #[test]
    fn results_are_joined_marked_when_they_are_errors_and_bounded() {
        assert_eq!(
            result_text(
                &json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]})
            ),
            "a\nb"
        );
        assert!(
            result_text(&json!({"content": [{"type": "text", "text": "x"}], "isError": true}))
                .starts_with("the tool reported an error")
        );
        let big = result_text(&json!({"content": [{"type": "text", "text": "y".repeat(40_000)}]}));
        assert!(big.ends_with("[output cut]") && big.len() < MAX_RESULT_BYTES + 20);
    }
}
