#!/usr/bin/env python3
"""Offline text-only Claude peer, with adversarial native capability modes."""
import json
from pathlib import Path
import sys
import time

if sys.argv[1:] == ["--help"]:
    print("--tools --strict-mcp-config --safe-mode --setting-sources --permission-prompts --no-session-persistence")
    sys.exit(0)
root = Path.cwd()
mode = (root / "mode").read_text() if (root / "mode").exists() else ""
with (root / "calls.jsonl").open("a") as output:
    output.write(json.dumps({"args": sys.argv[1:], "prompt": sys.stdin.read()}) + "\n")


def emit(value):
    print(json.dumps(value), flush=True)


emit({"type": "system", "subtype": "init", "model": "selected-model",
      "tools": ["Bash"] if mode == "tools" else [], "mcp_servers": []})
if mode == "delay":
    while True:
        time.sleep(1)
elif mode == "tool-call":
    emit({"type": "stream_event", "event": {"type": "content_block_start", "content_block": {"type": "tool_use", "name": "Bash"}}})
elif mode == "error":
    emit({"type": "result", "subtype": "error_during_execution", "is_error": True,
          "result": "api_key=secret-fixture-only"})
else:
    emit({"type": "stream_event", "event": {"type": "content_block_delta",
          "delta": {"type": "text_delta", "text": "frozen answer"}}})
    emit({"type": "assistant", "message": {"content": [{"type": "text", "text": "frozen answer"}]}})
    emit({"type": "result", "subtype": "success", "is_error": False, "result": "frozen answer"})
