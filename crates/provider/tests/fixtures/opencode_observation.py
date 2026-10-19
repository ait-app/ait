"""Authenticated read-only native metadata fixture; ACP continues to own every mutation."""
import base64
import json
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit


def serve(root, port, state_path, session_id):
    expected = "Basic " + base64.b64encode(
        ("opencode:" + os.environ["OPENCODE_SERVER_PASSWORD"]).encode()
    ).decode()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            if self.headers.get("Authorization") != expected:
                self.send_error(401)
                return
            path = urlsplit(self.path).path
            with (root / "http-requests.jsonl").open("a") as log:
                log.write(json.dumps({"method": "GET", "path": path}) + "\n")
            if path == "/event":
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                try:
                    while True:
                        self.wfile.write(b'data: {"type":"server.connected"}\n\n')
                        self.wfile.flush()
                        time.sleep(1)
                except (BrokenPipeError, ConnectionResetError):
                    pass
                return
            if path == "/config/providers":
                value = {"providers": [{"id": "local", "models": {
                    "model": {"limit": {"context": 32000}},
                    "second": {"limit": {"context": 64000}},
                }}]}
            elif path == "/session/" + session_id():
                value = {"id": session_id(), "cost": 0.02}
            elif path == "/session/" + session_id() + "/message":
                saved = json.loads(state_path().read_text())
                value = []
                for index, row in enumerate(saved["history"]):
                    info = {"id": row["id"], "sessionID": session_id(),
                            "role": "user" if row["type"] == "user" else "assistant",
                            "time": {"created": index + 1}, "providerID": "local", "modelID": "model",
                            "tokens": {"input": 100, "cache": {"read": 0}}}
                    value.append({"info": info, "parts": []})
            else:
                self.send_error(404)
                return
            data = json.dumps(value).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
