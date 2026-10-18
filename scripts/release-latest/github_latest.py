#!/usr/bin/env python3
"""Point the rolling `latest` GitHub release at a published stable Ait release.

The `latest` release holds fixed-name copies (Ait-macos-arm64.dmg,
Ait-daemon-*.tar.gz) and their SHA256SUMS, so
releases/download/latest/<name> never needs a version. It is published with
--latest=false so GitHub's Latest badge and the desktop updater keep using the
versioned release. New files are uploaded under a staging name first and only
then swapped in, so each download link is missing for at most one rename.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from common import latest_assets, stable_version, verified_files

TAG = "latest"
STAGING = ".next"
MARKER = re.compile(r"<!-- ait-latest-version: ([0-9.]+) -->")


def gh(*arguments):
    result = subprocess.run(["gh", *arguments], capture_output=True, text=True)
    if result.returncode:
        raise ValueError(f"gh {' '.join(arguments[:3])} failed: {result.stderr.strip()}")
    return result.stdout


class Latest:
    def __init__(self, repository, version, run=gh):
        self.repository, self.version, self.run = repository, version, run

    def api(self, endpoint, *arguments):
        output = self.run("api", f"repos/{self.repository}/{endpoint}", *arguments)
        return json.loads(output) if output.strip() else None

    def optional(self, endpoint):
        try:
            return self.api(endpoint)
        except ValueError as error:
            if "HTTP 404" in str(error):
                return None
            raise

    def release(self):
        return self.optional(f"releases/tags/{TAG}")

    def tag_exists(self):
        return self.optional(f"git/ref/tags/{TAG}") is not None

    def assets(self, release):
        return {asset["name"]: asset for asset in self.api(f"releases/{release['id']}/assets?per_page=100")}

    def notes(self):
        tag = f"v{self.version}"
        return (f"Fixed-name copies of the latest stable release, [{tag}](https://github.com/{self.repository}/releases/tag/{tag}). "
                "Replaced automatically on each stable release; see that release for notes and the full asset list.\n\n"
                f"<!-- ait-latest-version: {self.version} -->\n")

    def move_tag(self, commit, exists):
        if exists:
            self.api(f"git/refs/tags/{TAG}", "-X", "PATCH", "-f", f"sha={commit}", "-F", "force=true")
        else:
            self.api("git/refs", "-X", "POST", "-f", f"ref=refs/tags/{TAG}", "-f", f"sha={commit}")


def stage(directory, files, version, destination):
    """Copy verified versioned files to their fixed names and write a matching SHA256SUMS."""
    expected = {}
    for name, source in latest_assets(version).items():
        shutil.copyfile(files[source]["path"], destination / name)
        expected[name] = files[source]["sha256"]
    sums = "".join(f"{digest}  {name}\n" for name, digest in expected.items())
    (destination / "SHA256SUMS").write_text(sums)
    expected["SHA256SUMS"] = hashlib.sha256(sums.encode()).hexdigest()
    return expected


def publish(repository, version, directory, run=gh):
    stable_version(version)
    files = verified_files(directory, version)
    latest = Latest(repository, version, run)
    commit = latest.api(f"commits/v{version}")["sha"]
    release = latest.release()
    if release is not None:
        held = MARKER.search(release.get("body") or "")
        if held and stable_version(held[1]) > stable_version(version):
            return {"status": f"kept-newer-{held[1]}"}
    with tempfile.TemporaryDirectory() as temporary:
        staging = Path(temporary)
        expected = stage(directory, files, version, staging)
        title = f"Ait latest (v{version})"
        if release is None:
            latest.move_tag(commit, exists=latest.tag_exists())
            run("release", "create", TAG, *(str(staging / name) for name in expected), "--verify-tag",
                "--title", title, "--notes", latest.notes(), "--latest=false", "--repo", repository)
            return {"status": "created", "commit": commit}
        current = latest.assets(release)
        held = MARKER.search(release.get("body") or "")
        if held and held[1] == version and all(
                current.get(name, {}).get("digest") == f"sha256:{digest}" for name, digest in expected.items()):
            return {"status": "already-current", "commit": commit}
        latest.move_tag(commit, exists=True)
        for name in expected:
            (staging / name).rename(staging / f"{name}{STAGING}")
        for name in current:
            if name.endswith(STAGING):
                latest.api(f"releases/assets/{current[name]['id']}", "-X", "DELETE")
        run("release", "upload", TAG, *(str(staging / f"{name}{STAGING}") for name in expected), "--repo", repository)
        uploaded = latest.assets(release)
        for name in expected:
            if name in uploaded:
                latest.api(f"releases/assets/{uploaded[name]['id']}", "-X", "DELETE")
            latest.api(f"releases/assets/{uploaded[name + STAGING]['id']}", "-X", "PATCH", "-f", f"name={name}")
        for name, asset in uploaded.items():
            if name not in expected and name.removesuffix(STAGING) not in expected:
                latest.api(f"releases/assets/{asset['id']}", "-X", "DELETE")
        run("release", "edit", TAG, "--title", title, "--notes", latest.notes(), "--repo", repository)
    return {"status": "updated", "commit": commit}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("repository", help="owner/name, for example ait-app/ait")
    parser.add_argument("version", help="Stable release version, for example 0.1.8")
    parser.add_argument("directory", type=Path, help="Directory holding the versioned assets and SHA256SUMS")
    args = parser.parse_args()
    print(json.dumps(publish(args.repository, args.version.removeprefix("v"), args.directory), indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
