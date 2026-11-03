"""A minimal MCP server over stdio, for tests: one tool, `echo`, that returns its argument."""
import json
import sys

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if "id" not in message:
        continue  # notifications get no answer
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "mock", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "echo", "description": "Return the text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}]}
    elif method == "tools/call":
        params = message["params"]
        if params["name"] != "echo":
            print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32602, "message": "unknown tool"}}), flush=True)
            continue
        text = params.get("arguments", {}).get("text", "")
        # A notification in the middle must not confuse the client.
        print(json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {"data": "log"}}), flush=True)
        result = {"content": [{"type": "text", "text": "echo: " + text}], "isError": False}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": "no such method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
