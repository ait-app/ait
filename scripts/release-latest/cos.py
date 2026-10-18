#!/usr/bin/env python3
"""Mirror a published stable Ait release to Tencent COS and refresh its latest objects.

Versioned objects (Ait-X.Y.Z-*) are write-once. Fixed-name latest objects
(Ait-macos-arm64.dmg, Ait-daemon-*.tar.gz) are overwritten only when --latest is
set and never with an older version than the one they already hold.
"""

import argparse
import json
import logging
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.request

from common import VERSION, latest_assets, stable_version, verified_files

IMMUTABLE_CACHE = "public, max-age=31536000, immutable"
LATEST_CACHE = "no-cache"


def target():
    bucket, region = os.environ.get("COS_BUCKET", ""), os.environ.get("COS_REGION", "")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]*-[0-9]+", bucket) or not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)+", region):
        raise ValueError("COS_BUCKET and COS_REGION must name the release bucket.")
    return bucket, region


def lower(headers):
    return {key.lower(): value for key, value in (headers or {}).items()}


def same_object(headers, file):
    headers = lower(headers)
    return str(headers.get("content-length")) == str(file["size"]) and headers.get("x-cos-meta-sha256") == file["sha256"]


def check_public_url(url, file):
    request = urllib.request.Request(url, method="HEAD")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status != 200 or not same_object(dict(response.headers), file):
                raise ValueError(f"Public COS object does not match the release asset: {url}")
    except urllib.error.HTTPError as error:
        raise ValueError(f"Public COS download check failed (HTTP {error.code}): {url}") from None
    except urllib.error.URLError:
        raise ValueError(f"Public COS download check could not connect: {url}") from None


class Mirror:
    def __init__(self, client, missing_error, bucket, region, version):
        self.client, self.missing_error = client, missing_error
        self.bucket, self.base = bucket, f"https://{bucket}.cos.{region}.myqcloud.com/"
        self.version = version

    def head(self, key):
        try:
            return self.client.head_object(Bucket=self.bucket, Key=key)
        except self.missing_error as error:
            # HEAD responses have no XML body; any status other than 404 fails closed.
            if error.get_status_code() != 404:
                raise
            return None

    def upload(self, key, file, filename, cache_control, immutable):
        self.client.forbid_overwrite = immutable
        self.client.upload_file(
            Bucket=self.bucket, Key=key, LocalFilePath=str(file["path"]), PartSize=16, MAXThread=4, EnableMD5=True,
            ContentType="application/octet-stream",
            ContentDisposition=f'attachment; filename="{filename}"',
            CacheControl=cache_control,
            Metadata={"x-cos-meta-sha256": file["sha256"], "x-cos-meta-release-version": self.version},
        )
        if not same_object(self.head(key), file):
            raise ValueError(f"COS object metadata differs after upload: {key}")

    def versioned(self, name, file):
        existing = self.head(name)
        if existing is not None:
            if not same_object(existing, file):
                raise ValueError(f"COS already holds different content for {name}; it is never overwritten.")
            status = "already-present"
        else:
            self.upload(name, file, name, IMMUTABLE_CACHE, immutable=True)
            status = "uploaded"
        check_public_url(self.base + name, file)
        return status

    def latest(self, name, file, source):
        existing = self.head(name)
        if existing is not None:
            held = lower(existing).get("x-cos-meta-release-version", "")
            if VERSION.fullmatch(held) and stable_version(held) > stable_version(self.version):
                return f"kept-newer-{held}"
            if held == self.version and same_object(existing, file):
                check_public_url(self.base + name, file)
                return "already-current"
        # Downloads keep the versioned filename, so users can tell which release they have.
        self.upload(name, file, source, LATEST_CACHE, immutable=False)
        check_public_url(self.base + name, file)
        return "updated"


def cos_client(region):
    from qcloud_cos import CosConfig, CosS3Client

    class ReleaseClient(CosS3Client):
        forbid_overwrite = True

        def send_request(self, method, url, **kwargs):
            params = kwargs.get("params", {})
            # SDK 1.9.44 upload_file drops extra headers at multipart completion, so the
            # write-once header is added at the transport hook, before the request is signed.
            if self.forbid_overwrite and ((method == "PUT" and "uploadId" not in params) or (method == "POST" and "uploadId" in params)):
                kwargs.setdefault("headers", {})["x-cos-forbid-overwrite"] = "true"
            return super().send_request(method, url, **kwargs)

    secret_id, secret_key = os.environ.get("COS_SECRET_ID", ""), os.environ.get("COS_SECRET_KEY", "")
    if not secret_id or not secret_key:
        raise ValueError("COS_SECRET_ID and COS_SECRET_KEY are required.")
    logger = logging.getLogger("qcloud_cos")
    logger.setLevel(logging.CRITICAL)
    logger.propagate = False
    return ReleaseClient(CosConfig(Region=region, SecretId=secret_id, SecretKey=secret_key, Scheme="https", Timeout=60), retry=3)


def mirror(directory, version, update_latest, client, missing_error, bucket, region):
    files = verified_files(directory, version)
    versioning = client.get_bucket_versioning(Bucket=bucket).get("Status", "Disabled")
    if versioning not in ("Disabled", ""):
        raise ValueError("Bucket versioning is enabled or suspended; the write-once header cannot be relied on.")
    target = Mirror(client, missing_error, bucket, region, version)
    # Every versioned object must be in place before any latest object moves.
    result = {"version": version, "versioned": {}, "latest": {}}
    for name, file in files.items():
        result["versioned"][name] = {"status": target.versioned(name, file), "url": target.base + name, "sha256": file["sha256"]}
    if update_latest:
        for name, source in latest_assets(version).items():
            result["latest"][name] = {"status": target.latest(name, files[source], source), "url": target.base + name}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("version", help="Stable release version, for example 0.1.8")
    parser.add_argument("directory", type=Path, help="Directory holding the versioned assets and SHA256SUMS")
    parser.add_argument("--latest", action="store_true", help="Also refresh the fixed-name latest objects")
    args = parser.parse_args()
    bucket, region = target()
    from qcloud_cos.cos_exception import CosClientError, CosServiceError
    from requests.exceptions import RequestException
    try:
        result = mirror(args.directory, args.version.removeprefix("v"), args.latest, cos_client(region),
                        CosServiceError, bucket, region)
    except CosServiceError as error:
        # Report only the status and request ID; SDK messages may echo signed request details.
        raise ValueError(f"COS request failed: HTTP {error.get_status_code()} {error.get_error_code()} "
                         f"request_id={error.get_request_id()}") from None
    except (CosClientError, RequestException):
        raise ValueError("COS client request failed; check the network and credentials.") from None
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
