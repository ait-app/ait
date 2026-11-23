#!/usr/bin/env python3
"""Record or replay the stdio of a JSON-lines provider CLI; a developer and test tool only.

An adapter spawns this file (through a link) in place of the native CLI. The link's directory
holds `replay.json`:

    {"mode": "record" | "replay", "transcript": "<path>",
     "substitutions": {"{{cwd}}": "<absolute path>", ...},
     "program": "<real CLI>", "env": {...}, "redact": ["<key>", ...]}   # record mode only

Record mode runs the real CLI behind a tee and appends every stdio frame to the transcript.
Replay mode plays the transcript back: it writes the recorded CLI output, checks that every frame
the adapter writes equals the recorded one, and maps request ids. Progress and the first
mismatch go to `replay.log` beside the link, which the Rust helper reads after the test.

Transcript lines (NDJSON):
    {"type": "meta", "format": "ait-stdio-replay", "version": 1, ...}   first line
    {"type": "spawn", "process": N, "argv": [...]}                      Nth launch, in order
    {"type": "stdin", "process": N, "frame": {...}}                     adapter -> CLI
    {"type": "stdout", "process": N, "frame": {...}}                    CLI -> adapter

Substitutions are applied to every JSON string: recording replaces each value with its
placeholder, replay replaces each placeholder with the value of the replaying test. Recording
also replaces the value of every object key listed in "redact" with "redacted", the local user
and host names with "user" and "host", and email addresses with "user@example.com". The string
"{{any}}" in an expected stdin frame matches any value.
"""
import getpass
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import threading

ANY = "{{any}}"
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")

root = Path(sys.argv[0]).parent
config = json.loads((root / "replay.json").read_text())
transcript = Path(config["transcript"])


def claim_process():
    """Return this launch's index; launches of one test are numbered in spawn order."""
    index = 0
    while True:
        try:
            os.close(os.open(root / f"process-{index}", os.O_CREAT | os.O_EXCL | os.O_WRONLY))
            return index
        except FileExistsError:
            index += 1


def rewrite(value, replace, redact=frozenset()):
    if isinstance(value, str):
        return replace(value)
    if isinstance(value, list):
        return [rewrite(item, replace, redact) for item in value]
    if isinstance(value, dict):
        return {key: "redacted" if key in redact else rewrite(item, replace, redact)
                for key, item in value.items()}
    return value


def substitute(pairs):
    """A string rewriter for (old, new) pairs, longest old value first."""
    pairs = sorted(((old, new) for old, new in pairs if old), key=lambda pair: -len(pair[0]))
    def replace(text):
        for old, new in pairs:
            text = text.replace(old, new)
        return text
    return replace


def append(lock, entry):
    line = json.dumps(entry, ensure_ascii=False, separators=(",", ":")) + "\n"
    with lock, transcript.open("a") as output:
        output.write(line)


def record():
    process = claim_process()
    pairs = [(value, key) for key, value in config.get("substitutions", {}).items()]
    identity = [(getpass.getuser(), "user"), (socket.gethostname(), "host")]
    to_placeholders = substitute(pairs)
    # Whole words only, so a short host or user name does not rewrite unrelated text.
    identity = [(re.compile(rf"\b{re.escape(old)}\b"), new) for old, new in identity if old]
    def scrub(text):
        text = to_placeholders(text)
        for pattern, new in identity:
            text = pattern.sub(new, text)
        return EMAIL.sub("user@example.com", text)
    redact = frozenset(config.get("redact", []))
    lock = threading.Lock()
    append(lock, {"type": "spawn", "process": process, "argv": rewrite(sys.argv[1:], scrub)})
    child = subprocess.Popen([config["program"], *sys.argv[1:]], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, env={**os.environ, **config.get("env", {})})

    def tee(source, sink, kind):
        for line in source:
            try:
                entry = {"frame": rewrite(json.loads(line), scrub, redact)}
            except ValueError:
                entry = {"raw": scrub(line.decode(errors="replace"))}
            # Log before forwarding so a reaction to this frame is always recorded after it.
            append(lock, {"type": kind, "process": process, **entry})
            sink.write(line)
            sink.flush()
        sink.close()

    forward = threading.Thread(target=tee, args=(sys.stdin.buffer, child.stdin, "stdin"),
                               daemon=True)
    forward.start()
    tee(child.stdout, sys.stdout.buffer, "stdout")
    sys.exit(child.wait())


def kind(frame):
    if "method" in frame:
        return "request" if "id" in frame else "notification"
    return "response"


def matches(expected, actual):
    if expected == ANY:
        return True
    if isinstance(expected, dict):
        return (isinstance(actual, dict) and expected.keys() == actual.keys()
                and all(matches(value, actual[key]) for key, value in expected.items()))
    if isinstance(expected, list):
        return (isinstance(actual, list) and len(expected) == len(actual)
                and all(matches(value, item) for value, item in zip(expected, actual)))
    return expected == actual


class Replay:
    def __init__(self, process):
        self.process = process
        self.log = (root / "replay.log").open("a")
        self.ids = {}

    def note(self, **entry):
        self.log.write(json.dumps({"process": self.process, **entry}) + "\n")
        self.log.flush()

    def fail(self, reason, expected=None, actual=None):
        self.note(mismatch=reason, expected=expected, actual=actual)
        sys.stderr.write(f"stdio replay mismatch in process {self.process}: {reason}\n")
        sys.exit(1)

    def emit(self, entry):
        if "raw" in entry:
            sys.stdout.write(entry["raw"])
        else:
            frame = entry["frame"]
            if kind(frame) == "response" and json.dumps(frame["id"]) in self.ids:
                frame = {**frame, "id": self.ids[json.dumps(frame["id"])]}
            sys.stdout.write(json.dumps(frame, ensure_ascii=False) + "\n")
        sys.stdout.flush()

    def receive(self, index, entry):
        line = sys.stdin.readline()
        if not line:
            self.fail("the adapter closed stdin before sending an expected frame",
                      expected=entry.get("frame"))
        try:
            actual = json.loads(line)
        except ValueError:
            self.fail("the adapter sent a frame that is not JSON", actual=line)
        expected = entry["frame"]
        if kind(expected) != kind(actual) or expected.get("method") != actual.get("method"):
            self.fail("unexpected frame", expected, actual)
        if kind(expected) == "response" and expected.get("id") != actual.get("id"):
            self.fail("response to an unexpected request id", expected, actual)
        without_id = lambda frame: {key: value for key, value in frame.items() if key != "id"}
        if not matches(without_id(expected), without_id(actual)):
            self.fail("frame differs from the recording", expected, actual)
        if kind(expected) == "request":
            self.ids[json.dumps(expected["id"])] = actual["id"]
        self.note(consumed=index)

    def run(self, spawn, entries):
        if spawn is None:
            self.fail("the adapter launched more processes than the recording")
        if spawn["argv"] != sys.argv[1:]:
            self.fail("launch arguments differ from the recording", spawn["argv"], sys.argv[1:])
        expected = sum(entry["type"] == "stdin" for entry in entries)
        index = 0
        if expected == 0:
            self.note(finished=0)
        for entry in entries:
            if entry["type"] == "stdout":
                self.emit(entry)
                continue
            self.receive(index, entry)
            index += 1
            # Noted before the trailing output, which the adapter may close the process on.
            if index == expected:
                self.note(finished=index)
        for line in sys.stdin:
            self.fail("the adapter sent a frame after the recording ended", actual=line.strip())


def replay():
    process = claim_process()
    to_values = substitute(config.get("substitutions", {}).items())
    spawn, entries = None, []
    for line in transcript.read_text().splitlines():
        entry = json.loads(line)
        if entry.get("process") != process:
            continue
        entry = rewrite(entry, to_values)
        if entry["type"] == "spawn":
            spawn = entry
        else:
            entries.append(entry)
    Replay(process).run(spawn, entries)


if __name__ == "__main__":
    {"record": record, "replay": replay}[config["mode"]]()
