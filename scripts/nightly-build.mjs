import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export function nightlyBuildLabel(sha, timestamp) {
  assert.match(sha, /^[0-9a-f]{40}$/, "Expected a full source commit");
  assert(Number.isSafeInteger(timestamp) && timestamp >= 0, "Invalid commit timestamp");
  const date = new Date(timestamp * 1000).toISOString().slice(0, 10);
  assert.match(date, /^\d{4}-\d{2}-\d{2}$/, "Invalid commit date");
  return `${sha.slice(0, 8)}-${date}`;
}

// Electron-builder still needs its package SemVer, but all public names use the build identity.
export function nightlyBuilderArgs(label) {
  assert.match(label, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid nightly build label");
  return [
    `-c.mac.artifactName=Ait-${label}-macos-\${arch}.\${ext}`,
    `-c.linux.artifactName=Ait-${label}-linux-\${arch}.\${ext}`,
    `-c.appImage.artifactName=Ait-${label}-linux-\${arch}.\${ext}`,
  ];
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === "identity") {
    const sha = process.env.GITHUB_SHA;
    assert.match(sha ?? "", /^[0-9a-f]{40}$/);
    const timestamp = Number(
      execFileSync("git", ["show", "-s", "--format=%ct", sha], { encoding: "utf8" }).trim(),
    );
    appendFileSync(process.env.GITHUB_OUTPUT, `label=${nightlyBuildLabel(sha, timestamp)}\n`);
  } else if (process.argv[2] === "builder-args") {
    console.log(nightlyBuilderArgs(process.argv[3]).join("\n"));
  } else {
    throw new Error("Usage: nightly-build.mjs identity | builder-args LABEL");
  }
}
