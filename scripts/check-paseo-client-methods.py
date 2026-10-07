"""Check Ait client methods against component-owned Rust metadata."""

import json
import re
import subprocess
from pathlib import Path

from rust_method_specs import read_method_specs

ROOT = Path(__file__).resolve().parents[1]
expected = read_method_specs()
client = "\n".join(
    (ROOT / "apps/mobile/src/runtime/rust-daemon" / file).read_text()
    for file in ["methods.ts", "relay-methods.ts", "transport.ts"]
)
actual = {
    name: (kind, method)
    for name, method, kind in re.findall(
        r'"([^"]+)":\s*\{\s*method:\s*"([^"]+)",\s*kind:\s*"([^"]+)"', client
    )
}
if not actual or any(
    method != name or expected.get(name) != kind
    for name, (kind, method) in actual.items()
):
    raise SystemExit("Frontend methods must use canonical Rust names as both keys and wire methods")

# Historical source names belong to the pinned audit snapshot, not the runtime catalog.
snapshot = json.loads(
    (ROOT / "scripts/fixtures/paseo/paseo-api-contracts.json").read_text()
)
for entry in snapshot["entries"]:
    if entry["excluded"]:
        continue
    name = entry["canonical"]
    kind = entry["kind"]
    if expected.get(name) != kind or actual.get(name) != (kind, name):
        raise SystemExit(f"Missing or mismatched Ait method: {name}")
legacy = {
    entry["name"]
    for entry in snapshot["entries"]
    if not entry["excluded"] and entry["name"] != entry["canonical"]
}
# The historical source name "ping" is also an ordinary command and SDK function name.
# Check it only in message discriminators; other legacy names are unambiguous.
ping_pattern = re.compile(r'(?:type|method|requestType)\s*:\s*[\'"]ping[\'"]')
files = subprocess.check_output(
    ["git", "ls-files", "apps"], cwd=ROOT, text=True
).splitlines()
violations = []
for file in files:
    path = ROOT / file
    if path.suffix not in {".ts", ".tsx", ".js", ".mjs", ".cjs"}:
        continue
    content = path.read_text()
    present = {name for name in legacy - {"ping"} if name in content}
    legacy_pattern = (
        re.compile(
            r"(?<![\w./-])(?:"
            + "|".join(map(re.escape, sorted(present)))
            + r")(?![\w./-])"
        )
        if present
        else None
    )
    if not present and not ping_pattern.search(content):
        continue
    for number, line in enumerate(content.splitlines(), 1):
        if (legacy_pattern and legacy_pattern.search(line)) or ping_pattern.search(line):
            violations.append(f"{file}:{number}: {line.strip()}")
if violations:
    raise SystemExit("Legacy Paseo operations remain in apps/:\n" + "\n".join(violations))
print(f"Verified {len(actual)} Ait client methods against {len(expected)} component declarations")
