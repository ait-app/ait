#!/usr/bin/env python3
"""Private worker protocol fixture; no model or network dependency."""
import json
from pathlib import Path
import sys
import time
import wave

init = json.loads(sys.stdin.readline())
directory = Path(init["directory"])
with (directory / "starts").open("a") as starts:
    starts.write("1\n")
behavior_file = directory / "behavior"
if behavior_file.exists() and behavior_file.read_text() == "invalid-ack":
    print("bad", flush=True)
    sys.exit(0)
print("ok", flush=True)
for line in sys.stdin:
    request = json.loads(line)
    source, output = Path(request["input"]), Path(request["output"])
    (directory / "last-input").write_text(str(source))
    behavior = behavior_file.read_text() if behavior_file.exists() else "ready"
    if behavior == "block":
        time.sleep(60)
    elif behavior == "malformed":
        output.write_bytes(b"{")
    elif behavior.startswith("oversized-"):
        limit = 16 * 1024 * 1024 if behavior == "oversized-audio" else 128 * 1024
        with output.open("wb") as target:
            target.truncate(limit + 1)
    elif behavior not in ("missing", "acknowledge-only"):
        if init["model"] == "SenseVoice":
            with wave.open(str(source)) as audio:
                assert audio.getframerate() == 16000
                assert audio.getsampwidth() == 2
            output.write_text(json.dumps({"text": "recognized offline", "language": "en"}))
        else:
            assert source.read_text() == "speak this"
            with wave.open(str(output), "wb") as audio:
                audio.setnchannels(1)
                audio.setsampwidth(2)
                audio.setframerate(24000)
                audio.writeframes(b"\0\0" * 120)
    print("ok", flush=True)
