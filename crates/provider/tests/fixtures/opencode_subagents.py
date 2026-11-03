"""Native ACP child notifications and durable task links, without a model service."""
import json


def task(child, parent):
    return {"type": "tool", "id": "task-" + child, "title": "task", "kind": "other",
            "input": {"description": "Explore " + child, "subagent_type": "explore", "prompt": "Inspect"},
            "output": {"metadata": {"sessionId": child, "parentSessionId": parent,
                                     "model": {"providerID": "local", "modelID": "model"}}}}


def run(root, parent, send, update, state):
    for child, owner, depth in [("ses_child", parent, 1), ("ses_nested", "ses_child", 2)]:
        base = {"rootSessionId": parent, "childSessionId": child, "parentSessionId": owner,
                "depth": depth, "title": "Explore " + child}
        def notify(event):
            send({"method": "opencode/session/child_update", "params": {**base, **event}})
        notify({"type": "status", "status": "created"})
        notify({"type": "status", "status": "running"})
        notify({"type": "update", "update": {"sessionUpdate": "agent_message_chunk",
                "messageId": "answer-" + child, "content": {"type": "text", "text": "CHILD " + child}}})
        notify({"type": "status", "status": "completed"})
        history = [{"id": "user-" + child, "type": "user", "text": "Inspect"},
                   {"id": "answer-" + child, "type": "assistant",
                    "content": [{"type": "text", "text": "CHILD " + child}]}]
        if depth == 1:
            history[-1]["content"].insert(0, task("ses_nested", child))
        (root / (child + ".json")).write_text(json.dumps({"seq": 1, "history": history}))
    state["history"].append({"id": "task-message", "type": "assistant", "content": [task("ses_child", parent)]})
    (root / "native-fixture.json").write_text(json.dumps(state))


def background(root, parent, send, state):
    send({"method":"opencode/session/child_update", "params":{"rootSessionId":parent,
          "childSessionId":"ses_child", "parentSessionId":parent, "depth":1, "type":"status", "status":"running"}})
    part = task("ses_child", parent)
    part["output"]["metadata"]["background"] = True
    state["history"].append({"id":"background-task", "type":"assistant", "content":[part]})
    (root / "native-fixture.json").write_text(json.dumps(state))


def finish_background(root, parent, send):
    base = {"rootSessionId":parent, "childSessionId":"ses_child", "parentSessionId":parent, "depth":1}
    send({"method":"opencode/session/child_update", "params":{**base, "type":"update", "update":{
        "sessionUpdate":"agent_message_chunk", "messageId":"child-answer", "content":{"type":"text", "text":"Background answer"}}}})
    send({"method":"opencode/session/child_update", "params":{**base,"type":"status","status":"completed"}})
