#!/usr/bin/env python3
"""Offline OpenCode 2.x HTTP fixture for real server process ownership tests."""
import base64
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

if sys.argv[1:] == ["--version"]:
    print("2.0.10")
    sys.exit(0)
assert sys.argv[1:] == ["serve", "--hostname", "127.0.0.1", "--port", "0"]
password = os.environ["OPENCODE_PASSWORD"]
auth = "Basic " + base64.b64encode(("opencode:" + password).encode()).decode()
root = Path(__file__).resolve().parent
store = root / "native-fixture.json"
state = json.loads(store.read_text()) if store.exists() else {"history": [], "seq": 0}
lock = threading.Lock()
with (root / "pids.txt").open("a") as file:
    file.write(str(os.getpid()) + "\n")

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        self.handle_api()

    def do_POST(self):
        self.handle_api()

    def do_PATCH(self):
        self.handle_api()

    def do_PUT(self):
        self.handle_api()

    def send(self, data, stream=False):
        body = data.encode() if stream else json.dumps(data).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream" if stream else "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_api(self):
        if self.headers.get("authorization") != auth:
            self.send_error(401)
            return
        path = self.path.split("?")[0]
        length = int(self.headers.get("content-length", 0))
        body = json.loads(self.rfile.read(length)) if length else {}
        if path == "/api/event":
            # Finite stream exercises reconnect and authoritative reconciliation.
            self.send('data: {"type":"server.connected"}\n\n', True)
            return
        with lock:
            if path == "/api/info":
                self.send({"data": {"version": "2.0.10"}})
            elif path == "/api/plugin":
                self.send({"data": [{"id": "config", "state": {"status": "active"}}]})
            elif path == "/api/model":
                self.send({"data": [{"providerID": "local", "id": "model", "name": "Local model", "enabled": True, "variants": []}]})
            elif path == "/api/session/active":
                self.send({"data": {}})
            elif path == "/api/session" and self.command == "POST":
                state["info"] = dict(body, id="ses_one")
                self.send({"data": state["info"]})
            elif path == "/api/session/ses_one" and self.command == "PATCH":
                state["info"]["permissions"] = body["permissions"]
                self.send(None)
            elif path == "/api/session/ses_one/model":
                state["info"]["model"] = body["model"]
                self.send(None)
            elif path == "/api/session/ses_one":
                self.send({"data": state["info"]})
            elif path == "/api/session/ses_one/message":
                self.send({"data": state["history"], "cursor": {"next": None}})
            elif path == "/api/session/ses_one/permission":
                self.send({"data": []})
            elif path == "/api/session/ses_one/prompt":
                state["seq"] += 1
                seq = state["seq"]
                state["history"].extend([
                    {"id": f"user{seq}", "type": "user", "text": body["text"], "metadata": body["metadata"], "time": {"created": seq * 10}},
                    {"id": f"answer{seq}", "type": "assistant", "content": [{"type": "text", "text": "authoritative answer"}], "time": {"created": seq * 10 + 1, "completed": seq * 10 + 2}},
                ])
                state["info"]["outcome"] = "succeeded"
                store.write_text(json.dumps(state))
                self.send(None)
            elif path == "/api/experimental/session/ses_one/log":
                event = {"type": "session.execution.succeeded", "created": state["seq"] * 10 + 3, "durable": {"aggregateID": "ses_one", "seq": state["seq"]}, "data": {"sessionID": "ses_one"}}
                self.send("data: " + json.dumps(event) + "\n\n" if state["seq"] else "", True)
            elif path.startswith("/api/experimental/session/ses_one/instructions/entries/"):
                self.send(None)
            elif path == "/api/session/ses_one/interrupt":
                self.send(None)
            else:
                self.send_error(404)

server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print(f"opencode server listening on http://127.0.0.1:{server.server_port}", flush=True)
server.serve_forever()
