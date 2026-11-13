#!/usr/bin/env python3
"""Owned native OpenCode transient API fake; no session or model API is involved."""
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import time

root = Path.cwd()
mode = (root / "mode").read_text() if (root / "mode").exists() else ""
args = sys.argv[1:]
port = int(args[args.index("--port") + 1])
expected = "Basic " + base64.b64encode(("opencode:" + os.environ["OPENCODE_PASSWORD"]).encode()).decode()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, value, status=200):
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(value).encode())

    def auth(self):
        if self.headers.get("Authorization") == expected:
            return True
        self.reply({}, 401)
        return False

    def do_GET(self):
        if not self.auth():
            return
        with (root / "requests.jsonl").open("a") as output:
            output.write(json.dumps({"method": "GET", "path": self.path, "authenticated": True}) + "\n")
        if self.path.startswith("/api/config"):
            self.reply([{"type":"document","info":{"plugins":["must-not-load"],"providers":{"test":{
                "name":"Test","package":"@opencode/ai/providers/openai-compatible",
                "settings":{"apiKey":"private-fixture-model-key"},
                "headers":{"authorization":"private-earlier-header"},
                "models":{"model":{"name":"Test model","variants":[],"limit":{"context":32000,"output":2000}}}
            }}}}, {"type":"document","info":{"providers":{"test":{
                "settings":{"baseURL":"http://local-model-fixture"},
                "headers":{"Authorization":"private-latest-header"}
            }}}}])
        else:
            self.reply({"paths": {} if mode == "unsupported" else {
                "/api/experimental/generate": {"post": {"operationId": "experimental.generate.text"}}
            }})

    def do_POST(self):
        if not self.auth():
            return
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with (root / "requests.jsonl").open("a") as output:
            output.write(json.dumps({"method": "POST", "path": self.path, "payload": payload, "authenticated": True}) + "\n")
        overlay = json.loads(os.environ["OPENCODE_CONFIG_CONTENT"])
        assert set(overlay) == {"providers"}
        config = overlay["providers"]["test"]
        assert config["settings"] == {"apiKey":"private-fixture-model-key","baseURL":"http://local-model-fixture"}
        assert config["headers"] == {"Authorization":"private-latest-header"}
        if self.path != "/api/experimental/generate":
            return self.reply({}, 404)
        if mode == "delay":
            time.sleep(60)
        if mode == "error":
            return self.reply({"message": "api_key=secret-fixture-only"}, 503)
        self.reply({"data": {"text": "frozen answer"}})


ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
