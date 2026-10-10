#!/usr/bin/env python3
"""Immutable, offline OpenCode ACP fixture; state stays in the temporary test root."""
import json
import os
import pathlib
import sys

root = pathlib.Path(os.environ.get("AIT_ACP_FIXTURE_ROOT", pathlib.Path(sys.argv[0]).parent))
state_path = root / "native-fixture.json"
log_path = root / "requests.jsonl"
with (root / "pids.txt").open("a") as output:
    output.write(str(os.getpid()) + "\n")
if sys.argv[1:] == ["--version"]:
    print(os.environ.get("AIT_ACP_VERSION", "2.0.26"))
    sys.exit(0)
if sys.argv[1:2] == ["models"]:
    print("local/model")
    if "--verbose" in sys.argv:
        print(json.dumps({"name": "Local model", "variants": {"high": {}, "disabled": {"disabled": True}}}))
        print("local/second")
        print(json.dumps({"name": "Second model"}))
    sys.exit(0)
if sys.argv[1:] == ["agent", "list"]:
    print('build (primary)\n  []\nplan (primary)\n  []\ncustom (all)\n  []\nsummary (primary)\n  []\nexplore (subagent)\n  []')
    sys.exit(0)
if sys.argv[1:3] == ["debug", "agent"]:
    print(json.dumps({"name": sys.argv[-1], "hidden": sys.argv[-1] == "summary", "description": "Native agent"}))
    sys.exit(0)
assert sys.argv[1:] == ["acp"], sys.argv
with (root / "launch.jsonl").open("a") as output:
    output.write(json.dumps({"config": json.loads(os.environ.get("OPENCODE_CONFIG_CONTENT", "{}")),
                             "question": os.environ.get("OPENCODE_ENABLE_QUESTION_TOOL")}) + "\n")
scenario = os.environ.get("AIT_ACP_SCENARIO", "normal")
session_id = "ses_one"
model = "local/model"
effort = "default"
mode = "build"
if scenario == "custom-mode":
    mode = "review"
active = None


def session_path(identity):
    return root / "native-fixture.json" if identity == "ses_one" else root / (identity + ".json")


def claim_session():
    # Concurrent discovery, title and agent processes share the root; only one may own ses_one.
    try:
        with session_path("ses_one").open("x"):
            return "ses_one"
    except FileExistsError:
        return "ses_query_" + str(os.getpid())


def state():
    if state_path.exists():
        return json.loads(state_path.read_text())
    return {"seq": 0, "history": []}


def save(value):
    state_path.write_text(json.dumps(value))


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def reply(identity, value):
    send({"id": identity, "result": value})


def update(value):
    send({"method": "session/update", "params": {"sessionId": "wrong" if scenario == "wrong-session" else session_id, "update": value}})


def options():
    if scenario == "missing-model":
        return [{"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "local/second",
                 "options": [{"value": "local/second", "name": "Second model"}]}]
    choices = [
        {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": model,
         "options": [{"value": "local/model", "name": "Local model"}, {"value": "local/second", "name": "Second model"}]},
        {"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": effort,
         "options": [{"value": value, "name": value} for value in ["default", "high"]]},
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": mode,
         "options": [{"value": value, "name": value} for value in (["review"] if scenario == "custom-mode" else ["build", "plan"])]},
    ]
    if scenario == "no-thinking" and model == "local/second":
        return [choice for choice in choices if choice["category"] != "thought_level"]
    return choices


def content(row):
    if row["type"] == "user":
        update({"sessionUpdate": "user_message_chunk", "messageId": row["id"], "content": {"type": "text", "text": row["text"]}})
    else:
        for part in row["content"]:
            if part["type"] == "text":
                update({"sessionUpdate": "agent_message_chunk", "messageId": row["id"], "content": part})
            elif part["type"] == "tool":
                update({"sessionUpdate": "tool_call", "toolCallId": part["id"], "title": "read", "kind": part.get("kind", "read"), "status": "pending", "rawInput": part.get("input", {"path": "file"})})
                update({"sessionUpdate": "tool_call_update", "toolCallId": part["id"], "status": "completed", "rawOutput": part["output"]})


def complete(stop="end_turn"):
    global active
    value = state()
    row = {"id": "answer" + str(value["seq"]), "type": "assistant", "content": [{"type": "text", "text": "authoritative answer"}]}
    if scenario == "large-tool":
        row["content"].insert(0, {"type": "tool", "id": "tool" + str(value["seq"]), "output": "x" * 1048576})
    if scenario == "wrapped-tool":
        row["content"].insert(0, {"type": "tool", "id": "tool" + str(value["seq"]), "kind": "execute", "input": {"command": "pwd"}, "output": {"metadata": {"exit": 0}, "output": "/work\n"}})
    value["history"].append(row)
    save(value)
    content(row)
    update({"sessionUpdate": "usage_update", "used": 100, "size": 32000, "cost": {"amount": 0.02, "currency": "USD"}})
    reply(active, {"stopReason": stop})
    active = None


for line in sys.stdin:
    message = json.loads(line)
    with log_path.open("a") as output:
        output.write(json.dumps(message) + "\n")
    method = message.get("method")
    identity = message.get("id")
    params = message.get("params", {})
    if method is None:
        if message["id"] == "question":
            assert message["result"]["action"] in ["accept", "decline", "cancel"]
        elif message["id"] == "permission":
            assert message["result"]["outcome"]["outcome"] == "selected"
        complete()
    elif method == "initialize":
        assert params["clientCapabilities"]["elicitation"]["form"] == {}
        if scenario == "hung":
            continue
        if scenario == "malformed":
            print("not json", flush=True)
            continue
        capabilities = {"list": {}, "resume": {}, "close": {}}
        if scenario == "load-only":
            capabilities.pop("resume")
        if scenario == "no-history":
            capabilities.pop("list")
        if not os.environ.get("AIT_ACP_VERSION", "2.0.26").startswith("1.") and scenario != "no-delete":
            capabilities["delete"] = {}
        reply(identity, {"protocolVersion": 1, "agentCapabilities": {"loadSession": scenario != "no-history",
            "promptCapabilities": {"image": True}, "sessionCapabilities": capabilities}})
    elif method in ["session/new", "session/resume", "session/load"]:
        assert pathlib.Path(params["cwd"]).is_absolute()
        assert params["mcpServers"] == []
        if method == "session/new":
            session_id = claim_session()
            state_path = session_path(session_id)
            save({"seq": 0, "history": []})
        if method in ["session/load", "session/resume"]:
            session_id = params["sessionId"]
            state_path = session_path(session_id)
        if method == "session/load":
            if scenario == "preview-failed":
                send({"id": identity, "error": {"code": -32000, "message": "Fixture replay unavailable"}})
                continue
            for row in state()["history"]:
                content(row)
        reply(identity, {"sessionId": session_id, "configOptions": options()})
    elif method == "session/list":
        value = state()
        rows = [] if not (root / "native-fixture.json").exists() else [{"sessionId": "ses_one",
            "cwd": params.get("cwd", str(pathlib.Path.cwd())), "title": "External session", "updatedAt": "2026-10-09T06:00:00Z"}]
        reply(identity, {"sessions": rows})
    elif method == "session/set_config_option":
        if params["configId"] == "model":
            model = params["value"]
        elif params["configId"] == "mode":
            mode = params["value"]
        else:
            effort = params["value"]
        reply(identity, {"configOptions": options()})
    elif method == "session/prompt":
        assert active is None
        active = identity
        value = state()
        value["seq"] += 1
        value["history"].append({"id": "user" + str(value["seq"]), "type": "user", "text": "\n".join(block.get("text", "image") for block in params["prompt"])})
        save(value)
        if scenario == "question":
            send({"id": "question", "method": "elicitation/create", "params": {"mode": "form", "sessionId": session_id, "toolCallId": "tool-question", "message": "Choose languages",
                "requestedSchema": {"type": "object", "properties": {"language": {"type": "array", "uniqueItems": True, "items": {"anyOf": [{"const": "rust", "title": "Rust, stable"}, {"const": "go", "title": "Go"}]}},
                    "language_custom": {"type": "string"}}, "required": ["language"]}}})
        elif scenario == "permission":
            send({"id": "permission", "method": "session/request_permission", "params": {"sessionId": session_id,
                "toolCall": {"toolCallId": "shell", "title": "shell", "rawInput": {"command": "pwd"}},
                "options": [{"optionId": "once", "kind": "allow_once", "name": "Allow once"}, {"optionId": "always", "kind": "allow_always", "name": "Always allow"}, {"optionId": "reject", "kind": "reject_once", "name": "Reject"}]}})
        elif scenario == "waiting":
            update({"sessionUpdate": "agent_message_chunk", "messageId": "answer-live", "content": {"type": "text", "text": "Working"}})
        elif scenario == "ambiguous":
            sys.exit(0)
        else:
            complete()
    elif method == "session/cancel":
        if active:
            complete("cancelled")
    elif method == "session/close":
        reply(identity, {})
    elif method == "session/delete":
        state_path.unlink(missing_ok=True)
        reply(identity, {})
    else:
        raise AssertionError(method)
