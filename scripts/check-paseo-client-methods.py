"""Check canonical Ait client methods against the authoritative Rust catalog."""

from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
rust = (ROOT / "crates/protocol/src/methods.rs").read_text()
client = (ROOT / "apps/mobile/src/runtime/rust-daemon/methods.ts").read_text()
entries = re.findall(
    r'(request|event|response)!\(\s*\w+,\s*"([^"]+)",\s*"([^"]+)"\s*\)', rust
)
expected = {canonical: kind for kind, _, canonical in entries}
actual = {
    name: (kind, method)
    for name, method, kind in re.findall(
        r'"([^"]+)":\s*\{\s*method:\s*"([^"]+)",\s*kind:\s*"([^"]+)"', client
    )
}
if actual != {name: (kind, name) for name, kind in expected.items()}:
    raise SystemExit("Frontend methods must use canonical Rust names as both keys and wire methods")

legacy = {source for _, source, canonical in entries if source != canonical}
# The catalog source name "ping" is also an ordinary command and SDK function name.
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
print(f"Verified {len(expected)} canonical Ait client methods and apps/ operation names")
