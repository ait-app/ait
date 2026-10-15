#!/usr/bin/env python3
"""Offline keychain command peer; never reads the system keychain."""
import json
import sys

args = sys.argv[1:]
assert args[0] == "find-generic-password"
assert args[args.index("-s") + 1] == "Claude Code-credentials"
if "-a" in args:
    account = args[args.index("-a") + 1]
    assert account in ["fixture.user", "claude-code-user"]
    if account != "fixture.user":
        sys.exit(1)
    plan = "pro"
else:
    plan = "max"
print(json.dumps({"claudeAiOauth": {"accessToken": "offline-keychain-fixture", "subscriptionType": plan}}))
