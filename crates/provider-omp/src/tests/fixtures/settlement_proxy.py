#!/usr/bin/env python3
"""Drop only settlement notifications from an unmodified, isolated official OMP peer."""
import json
import os
import subprocess
import sys
import threading

peer = subprocess.Popen(
    [os.environ['AIT_TEST_OMP_REAL_BIN'], *sys.argv[1:]],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
)

def forward_input():
    try:
        for line in sys.stdin.buffer:
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
        if frame.get('type') == 'session_settled':
            continue
        if frame.get('type') == 'prompt_result':
            frame['sessionSettled'] = False
            line = (json.dumps(frame) + '\n').encode()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
finally:
    peer.wait()
