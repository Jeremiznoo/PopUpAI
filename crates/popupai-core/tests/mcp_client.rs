//! The MCP client against a small real MCP server (a Python script), over real pipes.

use std::collections::BTreeMap;

use popupai_core::mcp::McpServer;
use popupai_core::mcp_client::McpConnection;
use serde_json::json;

fn mock() -> McpServer {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mock_mcp.py");
    McpServer {
        id: "test/mock".into(),
        name: "mock".into(),
        source: "test",
        command: "python3".into(),
        args: vec!["-I".into(), script.into()],
        env: BTreeMap::new(),
    }
}

#[tokio::test]
async fn the_client_initialises_lists_tools_and_calls_one() {
    let connection = McpConnection::start(&mock())
        .await
        .expect("the server starts");
    assert_eq!(connection.tools.len(), 1);
    assert_eq!(connection.tools[0].name, "echo");
    assert_eq!(connection.tools[0].schema["required"][0], "text");

    let out = connection.call("echo", json!({"text": "hello"})).await;
    assert_eq!(
        out, "echo: hello",
        "a notification in between does not confuse it"
    );
    let again = connection.call("echo", json!({"text": "twice"})).await;
    assert_eq!(again, "echo: twice");
}

#[tokio::test]
async fn an_error_from_the_server_is_text_for_the_model() {
    let connection = McpConnection::start(&mock()).await.unwrap();
    let out = connection.call("nope", json!({})).await;
    assert!(out.contains("unknown tool"), "{out}");
}

#[tokio::test]
async fn a_program_that_is_not_a_server_fails_cleanly() {
    let mut server = mock();
    server.command = "definitely-not-a-program".into();
    assert!(McpConnection::start(&server).await.is_err());

    // A program that starts but never speaks MCP closes its output: reported, not hung.
    let mut silent = mock();
    silent.command = "true".into();
    silent.args.clear();
    let error = McpConnection::start(&silent).await.err().expect("it fails");
    assert!(error.contains("closed"), "{error}");
}
