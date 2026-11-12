#!/usr/bin/env python3
"""Give a sandbox command a real controlling PTY, then return its exit status."""
import errno
import os
import pty
import sys

pid, master = pty.fork()
if pid == 0:
    descriptor = os.open("/dev/tty", os.O_RDWR)
    os.close(descriptor)
    os.execvp(sys.argv[1], sys.argv[1:])
try:
    while True:
        try:
            chunk = os.read(master, 4096)
        except OSError as error:
            if error.errno == errno.EIO:
                break
            raise
        if not chunk:
            break
        sys.stdout.buffer.write(chunk)
finally:
    os.close(master)
_, status = os.waitpid(pid, 0)
sys.exit(os.waitstatus_to_exitcode(status))
