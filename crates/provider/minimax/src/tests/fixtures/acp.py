#!/usr/bin/env python3
"""Offline MiniMax Code ACP fixture with durable native conversation identity."""
import json
import os
import sys
import time
from pathlib import Path

assert sys.argv[1] == "acp"
root = Path.cwd()
scenario = (root / "scenario").read_text() if (root / "scenario").exists() else "normal"
# Native 0.7.7 lists each model per reasoning variant; non-thinking Flash is rejected.
FLASH_OFF = "m:minimax:MiniMax-M3.1-Flash:v:"
FLASH_THINKING = "m:minimax:MiniMax-M3.1-Flash:v:thinking"
VARIANTS = [
    {"value": FLASH_OFF, "name": "M3.1-Flash"},
    {"value": FLASH_THINKING, "name": "M3.1-Flash \u00b7 thinking"},
    {"value": "m:minimax:MiniMax-M3:v:", "name": "M3"},
    {"value": "m:minimax:MiniMax-M3:v:thinking", "name": "M3 \u00b7 thinking"},
]
model = FLASH_THINKING if scenario == "variants" else "minimax/flash"
mode = "default"
permission_mode = "auto"
thinking_effort = "default"
active = None
session_id = "minimax-session"
# Like native ACP agents, a prompt blocked on an unanswered permission cannot settle a cancel.
awaiting_permission = False
awaiting_questionnaire = False


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
    if scenario == "rejected-model":
        result[1]["options"].insert(0, {"value": "minimax/rejected", "name": "Unavailable variant"})
    if scenario in ("large-catalog", "slow-catalog"):
        result[1]["options"].extend(
            {"value": f"minimax/pro-{index}", "name": f"Pro {index}"} for index in range(140)
        )
    if scenario == "variants":
        result[1]["options"] = VARIANTS
    if model in ("minimax/flash", FLASH_THINKING):
        result.append({"id": "thinkingEffort", "category": "thought_level", "type": "select",
                       "name": "Thinking effort", "currentValue": thinking_effort, "options": [
                           {"value": "default", "name": "Default"},
                           {"value": "low", "name": "Low"}]})
    elif scenario == "alternate-effort":
        result.append({"id": "thinkingEffort", "category": "thought_level", "type": "select",
                       "name": "Thinking effort", "currentValue": "high", "options": [
                           {"value": "high", "name": "High"}]})
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
        assert params["clientCapabilities"]["elicitation"]["form"] == {}
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
            if params["value"] in ("minimax/rejected", FLASH_OFF):
                send({"id": identity, "error": {"code": -32603, "message": "Internal error",
                                                "data": {"details": "Invalid model reasoning"}}})
                continue
            if scenario == "catalog-disconnected":
                sys.exit(0)
            if scenario == "slow-catalog":
                time.sleep(0.02)
            model = params["value"]
        elif params["configId"] == "thinkingEffort":
            assert model in ("minimax/flash", FLASH_THINKING)
            thinking_effort = params["value"]
        else:
            permission_mode = params["value"]
        update({"sessionUpdate": "config_option_update", "configOptions": options()})
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
            awaiting_permission = True
        elif text == "questionnaire":
            send({"id": "question", "method": "elicitation/create", "params": {
                "sessionId": session_id, "toolCallId": "tool-question", "mode": "form",
                "message": "Choose languages", "requestedSchema": {"type": "object",
                    "properties": {
                        "language": {"type": "array", "uniqueItems": True, "items": {"anyOf": [
                            {"const": "rust", "title": "Rust, stable"},
                            {"const": "go", "title": "Go"}]}},
                        "language_custom": {"type": "string", "title": "Other language"}},
                    "required": ["language"]}}})
            awaiting_questionnaire = True
        elif text == "ask-user":
            # The frames native 0.7.7 sends for `ask_user`: the tool input, then a form with a
            # numeric identity, no `toolCallId` and a free-form `__other` property per step.
            steps = [{"id": "storage", "header": "Storage", "question": "Which database?",
                      "options": [{"id": "pg", "label": "Postgres", "recommended": True},
                                  {"id": "lite", "label": "SQLite"}]},
                     {"id": "language", "header": "Language", "question": "Which language?",
                      "options": [{"id": "rust", "label": "Rust"}, {"id": "go", "label": "Go"}]}]
            raw = {"mode": "questionnaire", "async": False, "title": "Project setup", "steps": steps}
            update({"sessionUpdate": "tool_call", "toolCallId": "call-ask", "title": "ask_user",
                    "name": "ask_user", "kind": "other"})
            update({"sessionUpdate": "tool_call_update", "toolCallId": "call-ask",
                    "status": "in_progress", "rawInput": raw})
            properties = {}
            for step in steps:
                properties[step["id"]] = {
                    "type": "string", "title": step["question"], "oneOf": [
                        {"const": option["id"], "title": option["label"],
                         "description": f"About {option['label']}"} for option in step["options"]]}
                properties[step["id"] + "__other"] = {
                    "type": "string", "title": step["question"] + " \u2014 Other",
                    "description": "Others..."}
            send({"id": 0, "method": "elicitation/create", "params": {
                "mode": "form", "sessionId": session_id, "message": "Project setup",
                "requestedSchema": {"type": "object", "properties": properties}}})
            update({"sessionUpdate": "tool_call_update", "toolCallId": "call-ask",
                    "status": "completed", "rawInput": raw, "rawOutput": {
                        "details": {"waiting_for_user": True}, "terminate": True}})
            awaiting_questionnaire = True
        elif text == "remember":
            (root / "native-memory").write_text("remembered")
            finish("remembered")
        elif text == "recall":
            finish((root / "native-memory").read_text())
        else:
            finish("Hello from MiniMax")
    elif method == "session/cancel":
        if scenario != "ignore-cancel" and not awaiting_permission and not awaiting_questionnaire:
            reply(active, {"stopReason": "cancelled"})
    elif method == "session/close":
        reply(identity, {})
    elif identity == "permission":
        awaiting_permission = False
        outcome = request["result"]["outcome"]
        if outcome["outcome"] == "cancelled":
            continue
        update({"sessionUpdate": "tool_call_update", "toolCallId": "tool", "status": "completed"})
        finish("approved" if outcome.get("optionId") == "yes" else "denied")
    elif identity == 0 and method is None:
        awaiting_questionnaire = False
        result = request["result"]
        if result["action"] == "cancel":
            continue
        assert result == {"action": "accept", "content": {
            "storage": "pg", "language__other": "Zig"}}, result
        finish("Postgres, Zig")
    elif identity == "question":
        awaiting_questionnaire = False
        result = request["result"]
        if result["action"] == "cancel":
            continue
        assert result["action"] == "accept"
        content = result["content"]
        assert content["language"] == ["rust", "go"]
        assert content["language_custom"] == "C++"
        finish("rust, go, C++")
    else:
        raise AssertionError(method)
