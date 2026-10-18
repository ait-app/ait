#!/usr/bin/env python3
"""Model Pi's detached native tool and signal-driven cleanup."""
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

if Path("ignore-term").exists():
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    print(json.dumps({"type": "ready", "pid": os.getpid()}), flush=True)
else:
    tool = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(30)"],
        start_new_session=True,
    )

    def cleanup(_signal, _frame):
        # Ensure a cancelled close or immediate runtime teardown cannot skip cleanup.
        time.sleep(0.15)
        os.killpg(tool.pid, signal.SIGTERM)
        tool.wait()
        Path("cleaned").write_text("done")
        sys.exit(0)

    signal.signal(signal.SIGTERM, cleanup)
    print(json.dumps({"type": "ready", "pid": tool.pid}), flush=True)

while True:
    time.sleep(1)
