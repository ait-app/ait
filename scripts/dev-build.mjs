import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { packagedVersion } from "./source-version.cjs";

export function devBuildLabel(sha, timestamp) {
  assert.match(sha, /^[0-9a-f]{40}$/, "Expected a full source commit");
  assert(Number.isSafeInteger(timestamp) && timestamp >= 0, "Invalid build timestamp");
  const date = new Date(timestamp * 1000).toISOString().slice(0, 10);
  assert.match(date, /^\d{4}-\d{2}-\d{2}$/, "Invalid build date");
  return `${sha.slice(0, 8)}-${date}`;
}

export function devDisplayVersion(label) {
  assert.match(label, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid dev build label");
  return `${label.slice(9)}-${label.slice(0, 8)}`;
}

export function devPackagedVersion(label) {
  return packagedVersion(devDisplayVersion(label));
}

// Package managers need SemVer internally; the display identity contains no release version.
export function devBuilderArgs(label) {
  assert.match(label, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid dev build label");
  return [
    `-c.extraMetadata.version=${devPackagedVersion(label)}`,
    "-c.publish.channel=latest",
    "-c.mac.publish.channel=latest",
    `-c.extraMetadata.displayVersion=${devDisplayVersion(label)}`,
    `-c.mac.artifactName=Ait-${label}-macos-\${arch}.\${ext}`,
    `-c.linux.artifactName=Ait-${label}-linux-\${arch}.\${ext}`,
    `-c.appImage.artifactName=Ait-${label}-linux-\${arch}.\${ext}`,
  ];
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === "identity") {
    const sha = process.env.GITHUB_SHA;
    assert.match(sha ?? "", /^[0-9a-f]{40}$/);
    const timestamp = Math.floor(Date.now() / 1000);
    appendFileSync(process.env.GITHUB_OUTPUT, `label=${devBuildLabel(sha, timestamp)}\n`);
  } else if (process.argv[2] === "builder-args") {
    console.log(devBuilderArgs(process.argv[3]).join("\n"));
  } else {
    throw new Error("Usage: dev-build.mjs identity | builder-args LABEL");
  }
}
