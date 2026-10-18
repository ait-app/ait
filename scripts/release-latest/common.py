"""Shared checks for publishing a stable Ait release's fixed-name latest downloads."""

import hashlib
import re

VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
DAEMON_TARGETS = ("linux-x64", "macos-arm64")


def stable_version(version):
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError(f"Only stable X.Y.Z releases publish latest downloads: {version}")
    return tuple(map(int, match.groups()))


def latest_assets(version):
    """Map each fixed-name latest download to its versioned release asset."""
    stable_version(version)
    return {
        "Ait-macos-arm64.dmg": f"Ait-{version}-macos-arm64.dmg",
        **{f"Ait-daemon-{target}.tar.gz": f"Ait-{version}-daemon-{target}.tar.gz" for target in DAEMON_TARGETS},
    }


def checksum_for(sums, name):
    matches = [match[1].lower() for line in sums.splitlines()
               if (match := re.fullmatch(r"([0-9a-fA-F]{64}) [ *](.+)", line)) and match[2] == name]
    if len(matches) != 1:
        raise ValueError(f"SHA256SUMS must list {name} exactly once.")
    return matches[0]


def verified_files(directory, version):
    """Check every versioned source against the release's SHA256SUMS before any publication."""
    sums = (directory / "SHA256SUMS").read_text()
    files = {}
    for source in latest_assets(version).values():
        path = directory / source
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {source}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != checksum_for(sums, source):
            raise ValueError(f"SHA-256 mismatch against SHA256SUMS: {source}")
        files[source] = {"path": path, "size": path.stat().st_size, "sha256": digest}
    return files
