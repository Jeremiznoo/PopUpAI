#!/usr/bin/env python3
"""A fake Ollama server for trying PopUpAI's interface without a real model.

Serves the two endpoints PopUpAI uses, on the loopback interface only:
  GET  /api/tags  -> one fake model
  POST /api/chat  -> a canned answer streamed word by word as newline-delimited JSON

It never logs request bodies, so nothing a user types is printed or stored.
Development tool only; not used by the automated tests.

    python3 tools/fake_ollama.py [--port 11555]
"""

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ANSWER = (
    "This is a canned answer from the fake Ollama server. It is streamed word by "
    "word so the popup can be checked without any real model: you should see the "
    "text appear progressively, and Stop should interrupt it."
)


class Handler(BaseHTTPRequestHandler):
    # HTTP/1.0: no keep-alive, the body simply ends when the connection closes.
    protocol_version = "HTTP/1.0"

    def log_message(self, fmt, *args):
        # Request line only (method and path); never the body.
        print(f"{self.command} {self.path}", flush=True)

    def do_GET(self):
        if self.path == "/api/tags":
            body = json.dumps({"models": [{"name": "fake-model:latest"}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_error(404)

    def do_POST(self):
        if self.path != "/api/chat":
            self.send_error(404)
            return
        # Read and discard the body: its content is private.
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.send_response(200)
        self.send_header("Content-Type", "application/x-ndjson")
        self.end_headers()
        try:
            for word in ANSWER.split(" "):
                line = {"message": {"role": "assistant", "content": word + " "}, "done": False}
                self.wfile.write((json.dumps(line) + "\n").encode())
                self.wfile.flush()
                time.sleep(0.07)
            done = {"message": {"role": "assistant", "content": ""}, "done": True}
            self.wfile.write((json.dumps(done) + "\n").encode())
        except (BrokenPipeError, ConnectionResetError):
            print("client disconnected", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=11555)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"fake Ollama on http://127.0.0.1:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
