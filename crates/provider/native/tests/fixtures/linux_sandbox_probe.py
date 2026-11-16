#!/usr/bin/env python3
"""Offline filesystem attack probes; all paths and credentials belong to the test fixture."""
import errno
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(sys.argv[2])
worktree = root / "wt"
home = root / "home"
common = root / "repo/.git"
private = common / "worktrees/wt"

if sys.argv[1] == "prepare":
    for directory in (worktree, home / ".codex", private):
        directory.mkdir(parents=True, exist_ok=True)
    (worktree / ".git").write_text(f"gitdir: {private}\n")
    (private / "commondir").write_text("../..\n")
    for name in ("overwrite", "append", "truncate", "unlink", "rename", "chmod", "hardlink", "read"):
        (home / name).write_text("original")
    (home / ".codex/auth.json").write_text("fixture-auth")
    (worktree / "outside").symlink_to(home, target_is_directory=True)
    sys.exit(0)

report = {"denied": [], "allowed": [], "unexpected": []}
blocked = {errno.EPERM, errno.EACCES, errno.EROFS, errno.EBUSY, errno.EXDEV}


def deny(name, action):
    try:
        action()
    except OSError as error:
        if error.errno in blocked:
            report["denied"].append(name)
        else:
            report["unexpected"].append([name, "wrong-error", error.errno])
    else:
        report["unexpected"].append([name, "allowed"])


def deny_command(name, command):
    result = subprocess.run(command, capture_output=True, text=True, timeout=5)
    if result.returncode != 0:
        report["denied"].append(name)
    else:
        report["unexpected"].append([name, "allowed"])


def append():
    with (home / "append").open("a") as output:
        output.write("changed")


def reopen_descriptor():
    with (home / "read").open("r") as source:
        Path(f"/proc/self/fd/{source.fileno()}").write_text("changed")


# The fixture owns these files and wrote them successfully before entering the sandbox.
assert (home / "read").read_text() == "original"
report["allowed"].append("read-outside")
(worktree / "allowed").write_text("inside")
(worktree / "allowed").rename(worktree / "renamed")
report["allowed"].append("write-and-rename-inside")
assert (home / ".codex/auth.json").read_text() == "fixture-auth"
(home / ".codex/auth.json.tmp").write_text("refreshed")
(home / ".codex/auth.json.tmp").replace(home / ".codex/auth.json")
report["allowed"].append("read-and-refresh-auth")

for name, target in (
    ("create-home", home / "new"),
    ("create-sibling", root / "new"),
    ("overwrite", home / "overwrite"),
    ("parent-traversal", worktree / "../home/new-parent"),
    ("symlink", worktree / "outside/new-symlink"),
    ("proc-root", Path("/proc/self/root") / home.relative_to("/") / "new-proc"),
    ("provider-config", home / ".codex/config.toml"),
    ("git-config", common / "config"),
    ("git-hook", common / "hooks/pre-commit"),
    ("git-private-config", private / "config.worktree"),
    ("git-pointer", worktree / ".git"),
):
    deny(name, lambda path=target: path.write_text("changed"))
deny("git-common-pointer", lambda: (private / "commondir").write_text(str(home)))
deny("delete-git-common-pointer", lambda: (private / "commondir").unlink())
deny("rename-git-common-pointer", lambda: (private / "commondir").rename(private / "old-common"))
deny("append", append)
deny("truncate", lambda: os.truncate(home / "truncate", 0))
deny("unlink", lambda: (home / "unlink").unlink())
deny("rename-outside", lambda: (home / "rename").rename(worktree / "stolen"))
deny("chmod", lambda: (home / "chmod").chmod(0o777))
deny("new-hardlink", lambda: os.link(home / "hardlink", worktree / "hardlink"))
deny("reopen-descriptor", reopen_descriptor)
deny("delete-config", lambda: (home / ".codex/config.toml").unlink())
deny("rename-config", lambda: (home / ".codex/config.toml").rename(home / ".codex/old"))
deny("rename-git-private", lambda: private.rename(private.with_name("moved")))
deny("rename-git-parent", lambda: private.parent.rename(private.parent.with_name("moved")))
deny_command("child-shell", ["sh", "-c", 'printf changed > "$1"', "probe", str(home / "child")])
deny_command("nested-userns", ["unshare", "--user", "--map-root-user", "true"])
deny_command("remount-root", ["mount", "-o", "remount,rw", "/"])

for name in ("overwrite", "append", "truncate", "unlink", "rename", "chmod", "hardlink", "read"):
    if not (home / name).exists() or (home / name).read_text() != "original":
        report["unexpected"].append([name, "outside-content-changed"])
if (private / "commondir").read_text() != "../..\n":
    report["unexpected"].append(["git-common-pointer", "changed-next-launch-policy"])
print(json.dumps(report, sort_keys=True))
