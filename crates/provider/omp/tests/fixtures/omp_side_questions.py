#!/usr/bin/env python3
"""Owned offline native BTW peer; never calls a model or reads user configuration."""
import json
from pathlib import Path
import sys
import threading
import time

root = Path.cwd()
mode = (root / "mode").read_text() if (root / "mode").exists() else ""
lock = threading.Lock()
record = None


def emit(value):
    with lock:
        print(json.dumps(value), flush=True)


def answer():
    time.sleep(0.025)
    if record["status"] != "running":
        return
    emit({"type": "btw_delta", "recordId": record["id"], "delta": "frozen answer"})
    if mode == "delay":
        return
    record["answer"] = "frozen answer"
    record["status"] = "complete"
    record["updatedAt"] += 1
    emit({"type": "btw_record", "record": record})


with (root / "args.jsonl").open("a") as output:
    output.write(json.dumps(sys.argv[1:]) + "\n")
emit({"type": "ready"})
for line in sys.stdin:
    request = json.loads(line)
    with (root / "requests.jsonl").open("a") as output:
        output.write(json.dumps(request) + "\n")
    kind = request["type"]
    response = {"type": "response", "command": kind, "id": request["id"], "success": True}
    if kind == "get_btw_history":
        if mode == "unsupported":
            response.update(success=False, error="Unsupported native command")
        else:
            response["data"] = {"records": []}
    elif kind == "btw":
        if mode == "reject":
            response.update(success=False, error="Native refusal")
        else:
            record = {"id": "native-topic", "question": request["question"], "answer": "",
                      "status": "running", "createdAt": 1, "updatedAt": 1,
                      "leafId": None, "followUps": []}
            response["data"] = {"record": record}
    elif kind == "btw_cancel":
        response["data"] = {"cancelled": record is not None and record["status"] == "running"}
        if response["data"]["cancelled"]:
            record["status"] = "cancelled"
            emit({"type": "btw_record", "record": record})
    else:
        response.update(success=False, error="No ordinary prompt or execution channel allowed")
    emit(response)
    if kind == "btw" and response["success"]:
        threading.Thread(target=answer, daemon=True).start()
