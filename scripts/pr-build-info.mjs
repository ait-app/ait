import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

export async function recordPrBuild(directory, env = process.env) {
  assert.match(env.PR_NUMBER ?? "", /^[1-9]\d*$/, "Missing PR number");
  for (const key of ["PR_HEAD_SHA", "PR_BASE_SHA", "GITHUB_SHA"])
    assert.match(env[key] ?? "", /^[0-9a-f]{40}$/, `Invalid ${key}`);
  assert(["linux", "mac"].includes(env.PLATFORM), "Invalid desktop platform");
  const info = {
    version: JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"))
      .version,
    channel: "pull-request",
    pullRequest: Number(env.PR_NUMBER),
    headCommit: env.PR_HEAD_SHA,
    baseCommit: env.PR_BASE_SHA,
    sourceCommit: env.GITHUB_SHA,
    platform: env.PLATFORM,
    workflowRunUrl: `${env.GITHUB_SERVER_URL}/${env.GITHUB_REPOSITORY}/actions/runs/${env.GITHUB_RUN_ID}`,
    macSigning: "ad-hoc, not notarized",
  };
  await writeFile(path.join(directory, "BUILD-INFO.json"), JSON.stringify(info, null, 2) + "\n");
  const sums = [];
  for (const file of (await readdir(directory)).filter((file) => file !== "SHA256SUMS").sort()) {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(path.join(directory, file))) hash.update(chunk);
    sums.push(`${hash.digest("hex")}  ${file}`);
  }
  await writeFile(path.join(directory, "SHA256SUMS"), sums.join("\n") + "\n");
  return info;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  assert.equal(process.argv.length, 3, "Usage: pr-build-info.mjs ASSETS_DIRECTORY");
  await recordPrBuild(process.argv[2]);
}
