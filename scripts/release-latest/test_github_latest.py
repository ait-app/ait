import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import common
import github_latest

REPO = "ait-app/ait"
COMMITS = {"v0.1.8": "8" * 40, "v0.1.9": "9" * 40}


class FakeGh:
    """Just enough of `gh` to model the rolling `latest` tag and release."""

    def __init__(self):
        self.tag, self.release, self.assets, self.calls, self.next_id = None, None, {}, [], 1

    def __call__(self, *arguments):
        self.calls.append(arguments)
        if arguments[0] == "api":
            return self.api(arguments[1].removeprefix(f"repos/{REPO}/"), arguments[2:])
        command, tag, *rest = arguments[1:]
        assert tag == "latest" and tuple(rest[-2:]) == ("--repo", REPO)
        if command == "create":
            assert self.release is None and self.tag and "--verify-tag" in rest and "--latest=false" in rest
            self.release = {"id": 1, "body": rest[rest.index("--notes") + 1]}
            self.upload([item for item in rest if item.startswith("/")])
        elif command == "upload":
            self.upload(rest[:-2])
        elif command == "edit":
            self.release["body"] = rest[rest.index("--notes") + 1]
        return ""

    def upload(self, paths):
        for path in map(Path, paths):
            assert path.name not in self.assets, f"upload would clobber {path.name}"
            body = path.read_bytes()
            self.assets[path.name] = {"id": self.next_id, "name": path.name, "body": body,
                                      "digest": "sha256:" + hashlib.sha256(body).hexdigest()}
            self.next_id += 1

    def by_id(self, asset_id):
        return next(name for name, asset in self.assets.items() if asset["id"] == asset_id)

    def api(self, endpoint, options):
        if endpoint.startswith("commits/"):
            return json.dumps({"sha": COMMITS[endpoint.removeprefix("commits/")]})
        if endpoint == "releases/tags/latest":
            if self.release is None:
                raise ValueError("gh api failed: HTTP 404: Not Found")
            return json.dumps(self.release)
        if endpoint == "git/ref/tags/latest":
            if self.tag is None:
                raise ValueError("gh api failed: HTTP 404: Not Found")
            return json.dumps({"object": {"sha": self.tag}})
        if endpoint in ("git/refs", "git/refs/tags/latest"):
            assert (self.tag is None) == (endpoint == "git/refs")
            self.tag = next(item.removeprefix("sha=") for item in options if item.startswith("sha="))
            return "{}"
        if endpoint == "releases/1/assets?per_page=100":
            return json.dumps([{key: value for key, value in asset.items() if key != "body"} for asset in self.assets.values()])
        asset_id = int(endpoint.removeprefix("releases/assets/"))
        name = self.by_id(asset_id)
        if "DELETE" in options:
            del self.assets[name]
        else:
            new = next(item.removeprefix("name=") for item in options if item.startswith("name="))
            assert new not in self.assets
            self.assets[new] = {**self.assets.pop(name), "name": new}
        return ""


class LatestReleaseTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.gh = FakeGh()

    def release(self, version):
        for path in self.directory.iterdir():
            path.unlink()
        lines = []
        for source in common.latest_assets(version).values():
            content = f"{source} contents".encode()
            (self.directory / source).write_bytes(content)
            lines.append(f"{hashlib.sha256(content).hexdigest()}  {source}")
        (self.directory / "SHA256SUMS").write_text("\n".join(lines) + "\n")

    def publish(self, version):
        self.release(version)
        return github_latest.publish(REPO, version, self.directory, self.gh)

    def fixed_names(self):
        return {name for name in self.gh.assets}

    def test_first_publish_creates_tag_and_release_with_fixed_names_and_sums(self):
        self.assertEqual(self.publish("0.1.8")["status"], "created")
        self.assertEqual(self.gh.tag, COMMITS["v0.1.8"])
        self.assertEqual(self.fixed_names(), {"Ait-macos-arm64.dmg", "Ait-daemon-linux-x64.tar.gz",
                                              "Ait-daemon-macos-arm64.tar.gz", "SHA256SUMS"})
        sums = self.gh.assets["SHA256SUMS"]["body"].decode()
        dmg = self.gh.assets["Ait-macos-arm64.dmg"]["body"]
        self.assertIn(f"{hashlib.sha256(dmg).hexdigest()}  Ait-macos-arm64.dmg\n", sums)
        self.assertEqual(dmg, b"Ait-0.1.8-macos-arm64.dmg contents")
        self.assertIn("<!-- ait-latest-version: 0.1.8 -->", self.gh.release["body"])

    def test_new_release_swaps_each_file_with_only_a_rename_gap(self):
        self.publish("0.1.8")
        self.gh.calls.clear()
        self.assertEqual(self.publish("0.1.9")["status"], "updated")
        self.assertEqual(self.gh.tag, COMMITS["v0.1.9"])
        self.assertEqual(self.gh.assets["Ait-daemon-linux-x64.tar.gz"]["body"], b"Ait-0.1.9-daemon-linux-x64.tar.gz contents")
        self.assertEqual(len(self.fixed_names()), 4)
        self.assertIn("v0.1.9", self.gh.release["body"])
        # Every fixed name disappears only between deleting the old asset and renaming the new one.
        swaps = [call for call in self.gh.calls if call[0] == "api" and "releases/assets/" in call[1]]
        for delete, rename in zip(swaps[::2], swaps[1::2]):
            self.assertIn("DELETE", delete)
            self.assertIn("PATCH", rename)
        upload = next(index for index, call in enumerate(self.gh.calls) if call[:2] == ("release", "upload"))
        first_delete = next(index for index, call in enumerate(self.gh.calls) if "DELETE" in call)
        self.assertLess(upload, first_delete)

    def test_rerun_with_the_same_release_changes_nothing(self):
        self.publish("0.1.8")
        self.gh.calls.clear()
        self.assertEqual(self.publish("0.1.8")["status"], "already-current")
        self.assertFalse(any(call[0] == "release" or "-X" in call for call in self.gh.calls))

    def test_older_release_never_replaces_a_newer_latest(self):
        self.publish("0.1.9")
        self.assertEqual(self.publish("0.1.8")["status"], "kept-newer-0.1.9")
        self.assertEqual(self.gh.tag, COMMITS["v0.1.9"])

    def test_interrupted_swap_recovers_on_rerun(self):
        self.publish("0.1.8")
        self.release("0.1.9")
        staged = Path(self.directory / "orphan.next")
        staged.write_bytes(b"partial")
        self.gh.upload([str(staged)])
        del self.gh.assets["Ait-macos-arm64.dmg"]
        github_latest.publish(REPO, "0.1.9", self.directory, self.gh)
        self.assertEqual(self.fixed_names(), {"Ait-macos-arm64.dmg", "Ait-daemon-linux-x64.tar.gz",
                                              "Ait-daemon-macos-arm64.tar.gz", "SHA256SUMS"})

    def test_checksum_mismatch_publishes_nothing(self):
        self.release("0.1.8")
        (self.directory / "Ait-0.1.8-macos-arm64.dmg").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            github_latest.publish(REPO, "0.1.8", self.directory, self.gh)
        self.assertEqual(self.gh.calls, [])


if __name__ == "__main__":
    unittest.main()
