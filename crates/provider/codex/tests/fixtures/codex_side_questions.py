#!/usr/bin/env python3
"""Owned app-server fault peer; no credentials, native accounts or model calls."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path.cwd()
thread = f"aux-{os.getpid()}"
turn = "side-turn"


def emit(frame):
    print(json.dumps(frame), flush=True)


def mode():
    path = root / "mode"
    return path.read_text().strip() if path.exists() else "complete"


def message(item, text):
    params = {"threadId": thread, "turnId": turn, "itemId": item}
    emit({"method": "item/agentMessage/delta", "params": {**params, "delta": text}})
    emit({"method": "item/completed", "params": {
        "threadId": thread, "turnId": turn,
        "item": {"type": "agentMessage", "id": item, "text": text}}})


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    params = request.get("params", {})
    with (root / "requests.jsonl").open("a") as log:
        log.write(json.dumps({**request, "pid": os.getpid()}) + "\n")
    if "id" not in request:
        continue
    result = {}
    if method == "initialize":
        version = root / "version"
        result = {"userAgent": "codex_cli_rs/" + (version.read_text().strip() if version.exists() else "0.167.0")}
    elif method == "config/read":
        result = {"config": {"mcp_servers": {"must-not-run": {"command": "false"}}}}
    elif method == "thread/start":
        assert params["ephemeral"] is True
        assert params["config"]["mcp_servers"]["must-not-run"]["enabled"] is False
        result = {"thread": {"id": thread, "model": params["model"], "ephemeral": True}}
    elif method == "turn/start":
        prompt = json.loads(params["input"][0]["text"])
        markers = list((root / "auxiliary").glob("request-*.json"))
        assert any(json.loads(path.read_text())["process_group"] == os.getpid() for path in markers)
        if mode() == "lost-ack":
            sys.exit(0)
        result = {"turn": {"id": turn}}
        if mode() == "hold-ack":
            time.sleep(30)
    elif method == "turn/interrupt":
        emit({"id": request["id"], "result": {}})
        emit({"method": "turn/completed", "params": {"threadId": thread, "turn": {"id": turn, "status": "interrupted"}}})
        continue
    else:
        emit({"id": request["id"], "error": {"code": -32601, "message": "unsupported fixture method"}})
        continue
    emit({"id": request["id"], "result": result})
    if method != "turn/start":
        continue
    if mode() == "hold":
        descendant = subprocess.Popen(["sleep", "30"])
        (root / "descendant").write_text(str(descendant.pid))
        message("partial", "visible partial")
        continue
    if mode() == "tool":
        emit({"method": "item/started", "params": {"threadId": thread, "turnId": turn, "item": {"type": "commandExecution", "id": "forbidden"}}})
        continue
    if mode() == "permission":
        emit({"id": "permission", "method": "item/commandExecution/requestApproval", "params": {"threadId": thread, "turnId": turn, "itemId": "forbidden"}})
        continue
    if mode() == "overflow":
        message("oversize", "x" * (48 * 1024 + 1))
        continue
    message("first", "first answer")
    message("second", "second answer")
    emit({"method": "turn/completed", "params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}})
