"""Read component-owned Rust method declarations for local protocol validation."""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DECLARATION = re.compile(
    r"const\s+\w+\s*:\s*&\[MethodSpec\]\s*=\s*&\[(.*?)\];", re.S
)
ENTRY = re.compile(
    r'MethodSpec::(request|event|response)\(\s*"([^"]+)"\s*,?\s*\)'
)


def read_method_specs(root=ROOT):
    """Return unique method names and directions from production component declarations."""
    methods = {}
    for path in sorted((root / "crates").glob("*/src/**/*.rs")):
        if "tests" in path.parts or path.name.endswith("tests.rs"):
            continue
        for declaration in DECLARATION.findall(path.read_text()):
            entries = ENTRY.findall(declaration)
            if not entries or ENTRY.sub("", declaration).replace(",", "").strip():
                raise ValueError(f"Unrecognized method metadata declaration: {path}")
            for kind, name in entries:
                if name in methods:
                    raise ValueError(f"Duplicate component method: {name} in {path}")
                methods[name] = kind
    if not methods:
        raise ValueError("No component-owned Rust methods found")
    return methods


if __name__ == "__main__":
    print(json.dumps(read_method_specs(), sort_keys=True))
