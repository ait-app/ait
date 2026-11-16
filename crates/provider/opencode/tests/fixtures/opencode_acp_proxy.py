#!/usr/bin/env python3
"""Drop one official ACP prompt result without changing native execution or history."""
import json
import os
import pathlib
import subprocess
import sys
import threading

real = os.environ['AIT_TEST_OPENCODE_REAL_BIN']
if sys.argv[1:2] != ['acp']:
    os.execv(real, [real, *sys.argv[1:]])

marker = pathlib.Path(os.environ['AIT_TEST_OPENCODE_DROP_MARKER'])
pending = set()
peer = subprocess.Popen([real, *sys.argv[1:]], stdin=subprocess.PIPE, stdout=subprocess.PIPE)

def forward_input():
    try:
        for line in sys.stdin.buffer:
            frame = json.loads(line)
            if frame.get('method') == 'session/prompt':
                pending.add(frame['id'])
            peer.stdin.write(line)
            peer.stdin.flush()
    except BrokenPipeError:
        pass
    finally:
        peer.stdin.close()

threading.Thread(target=forward_input, daemon=True).start()
try:
    for line in peer.stdout:
        frame = json.loads(line)
        if frame.get('id') in pending and 'method' not in frame:
            pending.discard(frame['id'])
            if 'result' in frame and not marker.exists():
                marker.touch()
                continue
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
finally:
    peer.wait()
