#!/usr/bin/env python3
"""Offline Cursor ACP contract fixture; no credentials or model requests."""
import json
import os
import sys
import threading
from pathlib import Path

assert sys.argv[1:] == ["acp"]
scenario = Path("scenario").read_text() if Path("scenario").exists() else ""
model, mode = "auto", "agent"
active = None
stage = 0
thinking, fast = "low", "false"

def send(message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)

def reply(identifier, result):
    send({"id": identifier, "result": result})

def parameters(selected=None):
    if (selected or model) == "auto":
        return []
    return [
        {"id": "thought_level", "category": "thought_level", "type": "select", "currentValue": thinking,
         "options": [{"value": value, "name": value.title()} for value in ["low", "high"]]},
        {"id": "fast", "type": "select", "currentValue": fast,
         "options": [{"value": "false", "name": "Off"}, {"value": "true", "name": "On"}]},
    ]

def options():
    return [
        {"id": "model", "category": "model", "type": "select", "currentValue": model,
         "options": [{"value": "auto", "name": "Auto"}, {"value": "composer", "name": "Composer"}]},
        {"id": "mode", "category": "mode", "type": "select", "currentValue": mode,
         "options": [{"value": value, "name": value.title()} for value in ["agent", "plan", "ask"]]},
    ] + parameters()

def state():
    if scenario not in ["legacy", "hybrid", "replay", "resume-only", "wrong-resume"]:
        return {"configOptions": options()}
    result = {
        "models": {"currentModelId": model, "availableModels": [
            {"modelId": "auto", "name": "Auto"}, {"modelId": "composer", "name": "Composer"}]},
        "modes": {"currentModeId": mode, "availableModes": [
            {"id": value, "name": value.title()} for value in ["agent", "plan", "ask"]]},
    }
    if scenario != "legacy":
        result["configOptions"] = parameters()
        result["models"]["availableModels"] = [{"modelId": model, "name": model.title()}]
    return result

def publish_commands():
    send({"method": "session/update", "params": {"sessionId": "native-one", "update": {
        "sessionUpdate": "available_commands_update", "availableCommands": [
            {"name": "compact", "description": "Compact context", "input": {"hint": "[instructions]"}}]}}})

def update(value):
    send({"method": "session/update", "params": {"sessionId": "foreign" if scenario == "foreign" else "native-one", "update": value}})

def callback():
    global stage
    stage += 1
    if stage == 1:
        send({"id": "permission", "method": "session/request_permission", "params": {
            "sessionId": "native-one", "toolCall": {"toolCallId": "tool-1", "title": "shell", "rawInput": {"command": "pwd"}},
            "options": [{"optionId": "once", "name": "Allow once", "kind": "allow_once"},
                        {"optionId": "deny", "name": "Reject", "kind": "reject_once"}]}})
    elif stage == 2:
        send({"id": 44, "method": "cursor/ask_question", "params": {"toolCallId": "question", "questions": [
            {"id": "language", "prompt": "Which language?", "options": [{"id": "rs", "label": "Rust"}, {"id": "ts", "label": "TypeScript"}]}]}})
    elif stage == 3:
        send({"id": "plan", "method": "cursor/create_plan", "params": {"toolCallId": "plan", "name": "Implementation", "plan": "Update the module", "todos": []}})
    else:
        update({"sessionUpdate": "tool_call_update", "toolCallId": "tool-1", "status": "completed", "rawOutput": "done"})
        for text in ["Hello ", "world"]:
            update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
        update({"sessionUpdate": "usage_update", "used": 123, "size": 1000})
        reply(active, {"stopReason": "end_turn", "usage": {"inputTokens": 42, "outputTokens": 7, "cachedReadTokens": 12}})

for line in sys.stdin:
    message = json.loads(line)
    with Path("requests.jsonl").open("a") as log:
        log.write(json.dumps({**message, "hasEnvironment": os.environ.get("CURSOR_TEST_ENV") == "ephemeral"}) + "\n")
    method, identifier, params = message.get("method"), message.get("id"), message.get("params", {})
    if method is None:
        if message.get("result", {}).get("outcome", {}).get("outcome") == "cancelled":
            continue
        if stage == 1:
            assert message["result"]["outcome"]["optionId"] == "once"
        elif stage == 2:
            assert message["result"]["outcome"]["answers"] == [{"questionId": "language", "selectedOptionIds": ["rs"]}]
        elif stage == 3:
            assert message["result"]["outcome"]["outcome"] == "accepted"
        callback()
    elif method == "initialize":
        assert params["protocolVersion"] == 1
        assert params["clientCapabilities"]["_meta"]["parameterizedModelPicker"] is True
        if scenario == "hung":
            continue
        if scenario == "malformed":
            print("bad json", flush=True)
            continue
        if scenario == "oversized":
            print("x" * (3 * 1024 * 1024), flush=True)
            continue
        reply(identifier, {"protocolVersion": 99 if scenario == "bad-version" else 1,
            "authMethods": [{"id": "cursor_login"}], "agentCapabilities": {
                "loadSession": scenario not in ["no-load", "resume-only"],
                "sessionCapabilities": {"resume": {}} if scenario == "resume-only" else {}, "promptCapabilities": {"image": scenario == "images"}}})
    elif method == "authenticate":
        assert params == {"methodId": "cursor_login"}
        if scenario == "unauthorized":
            send({"id": identifier, "error": {"code": -32000, "message": "Not logged in"}})
        else:
            reply(identifier, {})
    elif method == "cursor/list_available_models":
        if scenario in ["missing-catalog", "catalog-error"]:
            send({"id": identifier, "error": {"code": -32601 if scenario == "missing-catalog" else -32602, "message": "Catalog unavailable"}})
        elif scenario == "malformed-catalog":
            reply(identifier, {"models": [{"value": "auto", "name": "Auto", "configOptions": {}}]})
        else:
            reply(identifier, {"models": [] if scenario == "empty-catalog" else [
                {"value": "auto", "name": "Auto", "configOptions": parameters("auto")},
                {"value": "composer", "name": "Composer", "configOptions": parameters("composer")}]})
    elif method in ["session/new", "session/load", "session/resume"]:
        assert params["mcpServers"] == []
        assert Path(params["cwd"]).is_absolute()
        if method in ["session/load", "session/resume"]:
            assert params["sessionId"] == "native-one"
            if scenario == "replay":
                update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Old history"}})
                update({"sessionUpdate": "tool_call", "toolCallId": "old-tool", "status": "completed"})
        reply(identifier, {"sessionId": "foreign" if scenario == "wrong-resume" and method != "session/new" else "native-one", **state()})
        if scenario != "no-commands":
            threading.Timer(0.03, publish_commands).start()
    elif method in ["session/set_config_option", "session/set_model", "session/set_mode"]:
        category = params.get("configId", "model" if method == "session/set_model" else "mode")
        value = params.get("value", params.get("modelId", params.get("modeId")))
        if category == "model":
            model = value
        elif category == "mode":
            mode = value
        elif category == "thought_level":
            thinking = value
        elif category == "fast":
            fast = value
        else:
            raise AssertionError(category)
        if method == "session/set_model":
            update({"sessionUpdate": "config_option_update", "configOptions": parameters()})
            reply(identifier, {})
        elif scenario == "legacy" and method == "session/set_mode":
            reply(identifier, {})
        else:
            reply(identifier, {"configOptions": parameters()} if method == "session/set_config_option" and scenario in ["legacy", "hybrid"] else state())
    elif method == "session/prompt":
        active = identifier
        if any(block.get("text") == "wait" for block in params["prompt"]):
            if scenario == "cancel-tools":
                update({"sessionUpdate": "tool_call", "toolCallId": "unfinished", "title": "shell", "status": "in_progress", "rawInput": {"command": "sleep"}})
            continue
        if scenario == "hybrid":
            update({"sessionUpdate": "config_option_update", "configOptions": parameters()})
            update({"sessionUpdate": "plan", "entries": [{"content": "Implement", "status": "in_progress", "priority": "high"}]})
        update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Thinking"}})
        update({"sessionUpdate": "tool_call", "toolCallId": "tool-1", "title": "shell", "status": "in_progress", "rawInput": {"command": "pwd"}})
        stage = 0
        callback()
    elif method == "session/cancel":
        assert identifier is None
        reply(active, {"stopReason": "cancelled"})
    else:
        send({"id": identifier, "error": {"code": -32601, "message": "Unsupported method"}})
