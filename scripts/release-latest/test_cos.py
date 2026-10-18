import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import common
import cos as mirror

BUCKET, REGION = "ait-release-1251719274", "ap-shanghai"
BASE = f"https://{BUCKET}.cos.{REGION}.myqcloud.com/"


class Missing(Exception):
    def get_status_code(self):
        return 404


class FakeCos:
    """In-memory bucket that honours x-cos-forbid-overwrite like COS."""

    def __init__(self, objects=None):
        self.objects = dict(objects or {})
        self.forbid_overwrite = True
        self.uploads = []

    def get_bucket_versioning(self, Bucket):
        return {}

    def head_object(self, Bucket, Key):
        if Key not in self.objects:
            raise Missing()
        return self.objects[Key]

    def upload_file(self, Bucket, Key, LocalFilePath, Metadata, CacheControl, ContentDisposition, **_):
        if self.forbid_overwrite and Key in self.objects:
            raise AssertionError(f"write-once object overwritten: {Key}")
        self.uploads.append({"key": Key, "cache": CacheControl, "disposition": ContentDisposition,
                             "immutable": self.forbid_overwrite})
        self.objects[Key] = {"Content-Length": str(Path(LocalFilePath).stat().st_size), **Metadata}


def public_head(client):
    def urlopen(request, timeout):
        response = mock.MagicMock(status=200, headers=client.objects[request.full_url.removeprefix(BASE)])
        response.__enter__.return_value = response
        return response
    return mock.patch("urllib.request.urlopen", side_effect=urlopen)


class MirrorTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.release("0.1.8")

    def release(self, version, corrupt=None):
        lines = []
        for source in common.latest_assets(version).values():
            content = f"{source} contents".encode()
            (self.directory / source).write_bytes(b"tampered" if source == corrupt else content)
            lines.append(f"{hashlib.sha256(content).hexdigest()}  {source}")
        (self.directory / "SHA256SUMS").write_text("\n".join(lines) + "\n")

    def run_mirror(self, client, version="0.1.8", latest=True):
        with public_head(client):
            return mirror.mirror(self.directory, version, latest, client, Missing, BUCKET, REGION)

    def test_uploads_versioned_write_once_then_overwritable_latest(self):
        client = FakeCos()
        result = self.run_mirror(client)
        self.assertEqual([upload["key"] for upload in client.uploads], [
            "Ait-0.1.8-macos-arm64.dmg", "Ait-0.1.8-daemon-linux-x64.tar.gz", "Ait-0.1.8-daemon-macos-arm64.tar.gz",
            "Ait-macos-arm64.dmg", "Ait-daemon-linux-x64.tar.gz", "Ait-daemon-macos-arm64.tar.gz",
        ])
        versioned, latest = client.uploads[:3], client.uploads[3:]
        self.assertTrue(all(upload["immutable"] and "immutable" in upload["cache"] for upload in versioned))
        self.assertTrue(all(not upload["immutable"] and upload["cache"] == "no-cache" for upload in latest))
        self.assertEqual(latest[0]["disposition"], 'attachment; filename="Ait-0.1.8-macos-arm64.dmg"')
        self.assertEqual(client.objects["Ait-macos-arm64.dmg"]["x-cos-meta-release-version"], "0.1.8")
        self.assertEqual(result["latest"]["Ait-daemon-linux-x64.tar.gz"]["status"], "updated")

    def test_rerun_reuses_identical_objects(self):
        client = FakeCos()
        self.run_mirror(client)
        client.uploads.clear()
        result = self.run_mirror(client)
        self.assertEqual(client.uploads, [])
        self.assertEqual({item["status"] for item in result["versioned"].values()}, {"already-present"})
        self.assertEqual({item["status"] for item in result["latest"].values()}, {"already-current"})

    def test_new_release_replaces_previous_latest(self):
        client = FakeCos()
        self.run_mirror(client, "0.1.8")
        self.release("0.1.9")
        self.run_mirror(client, "0.1.9")
        self.assertEqual(client.objects["Ait-macos-arm64.dmg"]["x-cos-meta-release-version"], "0.1.9")
        self.assertIn("Ait-0.1.8-macos-arm64.dmg", client.objects)

    def test_older_release_never_moves_latest_backwards(self):
        client = FakeCos()
        self.release("0.1.9")
        self.run_mirror(client, "0.1.9")
        self.release("0.1.8")
        result = self.run_mirror(client, "0.1.8")
        self.assertEqual(client.objects["Ait-macos-arm64.dmg"]["x-cos-meta-release-version"], "0.1.9")
        self.assertEqual(result["latest"]["Ait-macos-arm64.dmg"]["status"], "kept-newer-0.1.9")
        self.assertIn("Ait-0.1.8-macos-arm64.dmg", client.objects)

    def test_without_latest_flag_only_versioned_objects_change(self):
        client = FakeCos()
        self.run_mirror(client, latest=False)
        self.assertNotIn("Ait-macos-arm64.dmg", client.objects)
        self.assertIn("Ait-0.1.8-macos-arm64.dmg", client.objects)

    def test_checksum_mismatch_uploads_nothing(self):
        self.release("0.1.8", corrupt="Ait-0.1.8-daemon-linux-x64.tar.gz")
        client = FakeCos()
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            self.run_mirror(client)
        self.assertEqual(client.uploads, [])

    def test_conflicting_versioned_object_stops_before_latest(self):
        client = FakeCos({"Ait-0.1.8-daemon-linux-x64.tar.gz": {"Content-Length": "1", "x-cos-meta-sha256": "0" * 64}})
        with self.assertRaisesRegex(ValueError, "never overwritten"):
            self.run_mirror(client)
        self.assertFalse(any(upload["key"].startswith("Ait-daemon") or upload["key"] == "Ait-macos-arm64.dmg"
                             for upload in client.uploads))

    def test_missing_or_duplicate_checksum_entries_are_rejected(self):
        sums = self.directory / "SHA256SUMS"
        lines = sums.read_text().splitlines()
        sums.write_text("\n".join(lines[:2]) + "\n")
        with self.assertRaisesRegex(ValueError, "exactly once"):
            self.run_mirror(FakeCos())
        sums.write_text("\n".join([*lines, lines[2]]) + "\n")
        with self.assertRaisesRegex(ValueError, "exactly once"):
            self.run_mirror(FakeCos())

    def test_beta_and_malformed_versions_are_refused(self):
        for version in ("0.1.8-beta.1", "v0.1.8", "0.1"):
            with self.assertRaisesRegex(ValueError, "Only stable"):
                common.latest_assets(version)


if __name__ == "__main__":
    unittest.main()
