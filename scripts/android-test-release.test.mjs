import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import yaml from "yaml";
import { releaseAssetNames } from "./release-assets.mjs";

const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/release-android.yml", import.meta.url), "utf8"),
);
const planStep = workflow.jobs.prepare.steps.find((step) => step.id === "plan");
const publishSteps = workflow.jobs.publish.steps;
const commit = "abcdef0123456789".repeat(2) + "abcdef01";

function run(script, cwd, env) {
  return spawnSync("bash", ["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script], {
    cwd,
    env: { ...process.env, ...env },
    encoding: "utf8",
  });
}

async function fixture(t) {
  const root = await mkdtemp(path.join(tmpdir(), "ait-android-preview-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const directory of [
    "scripts",
    "apps/mobile",
    "release-assets",
    "bin",
    ".tmp/release-tools/scripts",
  ])
    await mkdir(path.join(root, directory), { recursive: true });
  for (const file of [
    "scripts/android-release.mjs",
    "scripts/release-assets.mjs",
    "scripts/release-version.mjs",
    "apps/mobile/native-release-version.js",
  ])
    await copyFile(new URL(`../${file}`, import.meta.url), path.join(root, file));
  await copyFile(
    new URL("../scripts/release-assets.mjs", import.meta.url),
    path.join(root, ".tmp/release-tools/scripts/release-assets.mjs"),
  );
  await copyFile(
    new URL("./release-version.mjs", import.meta.url),
    path.join(root, ".tmp/release-tools/scripts/release-version.mjs"),
  );
  await writeFile(
    path.join(root, "apps/mobile/package.json"),
    JSON.stringify({ version: "0.0.14" }),
  );
  await writeFile(path.join(root, "release-assets/Ait-0.0.14-android.apk"), "universal APK");
  await writeFile(path.join(root, "bin/git"), `#!/bin/sh\necho ${commit}\n`, { mode: 0o700 });
  return root;
}

test("both modes share the Android build and publish only after APK validation", () => {
  assert.equal(workflow.permissions.contents, "read");
  assert.equal(
    workflow.jobs.prepare.steps.find((step) => step.name === "Check out release source").with.ref,
    "${{ inputs.mode == 'release' && format('refs/tags/{0}', inputs.tag) || github.sha }}",
  );
  assert.equal(
    workflow.jobs.build.steps.find((step) => step.name === "Check out release source").with.ref,
    "${{ needs.prepare.outputs.source_commit }}",
  );
  assert.deepEqual(workflow.jobs.publish.needs, ["prepare", "build"]);
  assert.equal(workflow.jobs.publish.permissions.contents, "write");
  for (const job of Object.values(workflow.jobs)) {
    for (const step of job.steps ?? []) {
      if (!step.run) continue;
      assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
      const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
      assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
    }
  }
});

test("release plan uses the checked-out commit and separates test tags from stable tags", async (t) => {
  const root = await fixture(t);
  const output = path.join(root, "outputs");
  const env = { GITHUB_OUTPUT: output, PATH: `${root}/bin:${process.env.PATH}` };
  const result = run(planStep.run, root, { ...env, MODE: "test", TAG: "" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    await readFile(output, "utf8"),
    `tag=v0.0.14\nsource_commit=${commit}\nrelease_tag=android-test-v0.0.14-${commit.slice(0, 12)}\n`,
  );
  await writeFile(output, "");
  assert.equal(run(planStep.run, root, { ...env, MODE: "release", TAG: "v0.0.14" }).status, 0);
  assert.equal(
    await readFile(output, "utf8"),
    `tag=v0.0.14\nsource_commit=${commit}\nrelease_tag=v0.0.14\n`,
  );
  assert.notEqual(run(planStep.run, root, { ...env, MODE: "release", TAG: "v0.0.15" }).status, 0);
});

test("test release metadata and checksums cover the universal APK and reject incomplete artifacts", async (t) => {
  const root = await fixture(t);
  const assets = path.join(root, "release-assets");
  const record = publishSteps.find((step) => step.name === "Verify assets and record test build");
  const env = {
    VERSION_TAG: "v0.0.14",
    SOURCE_COMMIT: commit,
    RELEASE_TAG: `android-test-v0.0.14-${commit.slice(0, 12)}`,
    GITHUB_WORKFLOW_SHA: commit,
    GITHUB_SERVER_URL: "https://github.com",
    GITHUB_REPOSITORY: "example/ait",
    GITHUB_RUN_ID: "123",
  };
  const result = run(record.run, root, env);
  assert.equal(result.status, 0, result.stderr);
  const info = JSON.parse(await readFile(path.join(assets, "BUILD-INFO.json"), "utf8"));
  assert.equal(info.sourceCommit, commit);
  assert.equal(info.releaseTag, env.RELEASE_TAG);
  assert.equal(info.signing, "eas-managed");
  const checksum = publishSteps.find((step) => step.name === "Write test APK checksums");
  assert.equal(run(checksum.run, assets, env).status, 0);
  assert.equal(run("sha256sum --check SHA256SUMS", assets, env).status, 0);
  await writeFile(path.join(assets, "Ait-0.0.14-android.apk"), "tampered");
  assert.notEqual(run("sha256sum --check SHA256SUMS", assets, env).status, 0);
  await rm(path.join(assets, "BUILD-INFO.json"));
  await rm(path.join(assets, "SHA256SUMS"));
  await rm(path.join(assets, "Ait-0.0.14-android.apk"));
  assert.notEqual(run(record.run, root, env).status, 0);
});

test("publishing creates a prerelease and only repairs a prerelease for the same commit", async (t) => {
  const root = await fixture(t);
  const log = path.join(root, "gh-calls.jsonl");
  await writeFile(
    path.join(root, "bin/gh"),
    `#!/usr/bin/env node
const fs = require('node:fs');
const args = process.argv.slice(2);
fs.appendFileSync(process.env.GH_TEST_LOG, JSON.stringify(args) + '\\n');
if (args[0] === 'release' && args[1] === 'view') {
  if (process.env.GH_TEST_EXISTS !== 'true') process.exit(1);
  if (args.includes('--json')) console.log(process.env.GH_TEST_PRERELEASE);
} else if (args[0] === 'api') console.log(process.env.GH_TEST_COMMIT);
`,
    { mode: 0o700 },
  );
  const step = publishSteps.find((value) => value.name === "Create or update test prerelease");
  for (const [exists, prerelease, source, expected] of [
    ["false", "true", commit, "create"],
    ["true", "true", commit, "upload"],
    ["true", "false", commit, null],
    ["true", "true", "0".repeat(40), null],
  ]) {
    await writeFile(log, "");
    const result = run(step.run, root, {
      PATH: `${root}/bin:${process.env.PATH}`,
      GH_TEST_LOG: log,
      GH_TEST_EXISTS: exists,
      GH_TEST_PRERELEASE: prerelease,
      GH_TEST_COMMIT: source,
      RELEASE_TAG: `android-test-v0.0.14-${commit.slice(0, 12)}`,
      SOURCE_COMMIT: commit,
      VERSION_TAG: "v0.0.14",
      GITHUB_REPOSITORY: "example/ait",
      GITHUB_SERVER_URL: "https://github.com",
      GITHUB_STEP_SUMMARY: path.join(root, "summary"),
    });
    const writes = (await readFile(log, "utf8"))
      .trim()
      .split("\n")
      .map(JSON.parse)
      .filter((args) => args[0] === "release" && ["create", "upload"].includes(args[1]));
    assert.equal(result.status === 0, expected !== null, result.stderr);
    assert.equal(writes.length, expected === null ? 0 : 1);
    if (expected !== null) assert.equal(writes[0][1], expected);
    if (expected === "create") {
      assert(writes[0].includes("--prerelease"));
      assert(writes[0].includes("--latest=false"));
      assert.equal(writes[0][writes[0].indexOf("--target") + 1], commit);
    }
  }
});

test("stable release verifies existing checksums and uploads only the APK and renewed sums", async (t) => {
  const root = await fixture(t);
  const published = path.join(root, "published-assets");
  await mkdir(published);
  const desktop = [
    ...releaseAssetNames("linux", "0.0.14"),
    ...releaseAssetNames("mac", "0.0.14"),
    "BUILD-INFO.json",
  ];
  const sums = [];
  for (const name of desktop) {
    const contents =
      name === "BUILD-INFO.json"
        ? JSON.stringify({
            version: "0.0.14",
            releaseTag: "v0.0.14",
            sourceCommit: commit,
            workflowCommit: commit,
          }) + "\n"
        : `original ${name}\n`;
    await writeFile(path.join(published, name), contents);
    sums.push(`${createHash("sha256").update(contents).digest("hex")}  ${name}`);
  }
  await writeFile(path.join(published, "SHA256SUMS"), `${sums.join("\n")}\n`);
  const log = path.join(root, "gh-calls.jsonl");
  await writeFile(
    path.join(root, "bin/gh"),
    `#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
fs.appendFileSync(process.env.GH_TEST_LOG, JSON.stringify(args) + '\\n');
if (args[0] === 'release' && args[1] === 'download') {
  const destination = args[args.indexOf('--dir') + 1];
  for (const name of fs.readdirSync(process.env.GH_TEST_RELEASE)) {
    fs.copyFileSync(path.join(process.env.GH_TEST_RELEASE, name), path.join(destination, name));
  }
}
`,
    { mode: 0o700 },
  );
  const env = {
    PATH: `${root}/bin:${process.env.PATH}`,
    GH_TEST_LOG: log,
    GH_TEST_RELEASE: published,
    VERSION_TAG: "v0.0.14",
    RELEASE_TAG: "v0.0.14",
    GITHUB_REPOSITORY: "example/ait",
    GITHUB_SERVER_URL: "https://github.com",
    GITHUB_STEP_SUMMARY: path.join(root, "summary"),
  };
  const prepare = publishSteps.find(
    (step) => step.name === "Download and verify stable release assets",
  );
  const upload = publishSteps.find((step) => step.name === "Add APK to stable release");
  const result = run(prepare.run, root, env);
  assert.equal(result.status, 0, result.stderr);
  const stable = path.join(root, "stable-assets");
  assert.equal(run("sha256sum --check SHA256SUMS", stable, env).status, 0);
  assert.equal(await readFile(path.join(stable, desktop[0]), "utf8"), `original ${desktop[0]}\n`);
  assert.equal(run(upload.run, root, env).status, 0);
  const calls = (await readFile(log, "utf8")).trim().split("\n").map(JSON.parse);
  const args = calls.find((value) => value[0] === "release" && value[1] === "upload");
  assert.deepEqual(args.slice(3, 5), [
    "stable-assets/Ait-0.0.14-android.apk",
    "stable-assets/SHA256SUMS",
  ]);
  await writeFile(path.join(published, desktop[0]), "tampered\n");
  assert.notEqual(run(prepare.run, root, env).status, 0);
});
