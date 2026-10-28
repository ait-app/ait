#!/usr/bin/env python3
"""Offline MiniMax Code ACP fixture with durable native conversation identity."""
import json
import os
import sys
from pathlib import Path

assert sys.argv[1] == "acp"
root = Path.cwd()
scenario = (root / "scenario").read_text() if (root / "scenario").exists() else "normal"
model = "minimax/flash"
mode = "default"
permission_mode = "auto"
thinking_effort = "default"
active = None
session_id = "minimax-session"


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def reply(identity, result):
    send({"id": identity, "result": result})


def options():
    result = [
        {"id": "permissionMode", "category": "_permission", "type": "select", "name": "Permissions",
         "currentValue": permission_mode, "options": [
             {"value": "default", "name": "Ask"}, {"value": "auto", "name": "Auto"},
             {"value": "bypassPermissions", "name": "Full access"}]},
        {"id": "model", "category": "model", "type": "select", "name": "Model",
         "currentValue": model, "options": [
             {"value": "minimax/flash", "name": "Flash"},
             {"value": "minimax/pro", "name": "Pro"}]},
    ]
    if model == "minimax/flash":
        result.append({"id": "thinkingEffort", "category": "thought_level", "type": "select",
                       "name": "Thinking effort", "currentValue": thinking_effort, "options": [
                           {"value": "default", "name": "Default"},
                           {"value": "low", "name": "Low"}]})
    return result


def update(data):
    send({"method": "session/update", "params": {
        "sessionId": "wrong" if scenario == "wrong-session" else session_id,
        "update": data}})


def finish(text):
    update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Thinking"}})
    update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
    reply(active, {"stopReason": "end_turn", "usage": {"inputTokens": 12, "outputTokens": 7}})


(root / "pid").write_text(str(os.getpid()))
for line in sys.stdin:
    request = json.loads(line)
    with (root / "requests.jsonl").open("a") as log:
        log.write(json.dumps(request) + "\n")
    method = request.get("method")
    identity = request.get("id")
    params = request.get("params", {})
    if method == "initialize":
        reply(identity, {"protocolVersion": 1, "agentCapabilities": {
            "promptCapabilities": {"image": True}, "mcpCapabilities": {"http": True},
            "sessionCapabilities": {"resume": {}, "close": {}}}})
    elif method in ("session/new", "session/resume"):
        if scenario == "auth":
            send({"id": identity, "error": {"code": -32000, "message": "Authentication required: run mcode login"}})
        else:
            if method == "session/resume":
                assert params["sessionId"] == session_id
            reply(identity, {"sessionId": session_id, "configOptions": options(), "modes": {"currentModeId":mode,"availableModes":[{"id":"default","name":"Default"},{"id":"plan","name":"Plan"}]}})
    elif method == "session/set_mode":
        mode = params["modeId"]
        reply(identity, {})
    elif method == "session/set_config_option":
        if params["configId"] == "model":
            model = params["value"]
        elif params["configId"] == "thinkingEffort":
            assert model == "minimax/flash"
            thinking_effort = params["value"]
        else:
            permission_mode = params["value"]
        reply(identity, {"configOptions": options()})
    elif method == "session/prompt":
        active = identity
        text = "".join(block.get("text", "") for block in params["prompt"])
        if text == "wait":
            continue
        if text == "fail":
            send({"id": active, "error": {"code": -32000, "message": "rate limit; api_key=sk-secret12345678"}})
        elif text == "approve":
            update({"sessionUpdate": "tool_call", "toolCallId": "tool", "title": "Read file", "status": "pending"})
            send({"id": "permission", "method": "session/request_permission", "params": {
                "sessionId": session_id, "toolCall": {"toolCallId": "tool", "title": "Read file"},
                "options": [{"optionId": "yes", "kind": "allow_once", "name": "Allow once"},
                            {"optionId": "no", "kind": "reject_once", "name": "Deny"}]}})
        elif text == "remember":
            (root / "native-memory").write_text("remembered")
            finish("remembered")
        elif text == "recall":
            finish((root / "native-memory").read_text())
        else:
            finish("Hello from MiniMax")
    elif method == "session/cancel":
        if scenario != "ignore-cancel":
            reply(active, {"stopReason": "cancelled"})
    elif method == "session/close":
        reply(identity, {})
    elif identity == "permission":
        outcome = request["result"]["outcome"]
        update({"sessionUpdate": "tool_call_update", "toolCallId": "tool", "status": "completed"})
        finish("approved" if outcome.get("optionId") == "yes" else "denied")
    else:
        raise AssertionError(method)
