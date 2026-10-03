import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import yaml from "yaml";

const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/release-android-test.yml", import.meta.url), "utf8"),
);
const planStep = workflow.jobs.prepare.steps.find((step) => step.id === "version");
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
  for (const directory of ["scripts", "apps/mobile", "release-assets", "bin"])
    await mkdir(path.join(root, directory), { recursive: true });
  for (const file of [
    "scripts/android-release.mjs",
    "scripts/release-assets.mjs",
    "apps/mobile/native-release-version.js",
  ])
    await copyFile(new URL(`../${file}`, import.meta.url), path.join(root, file));
  await writeFile(
    path.join(root, "apps/mobile/package.json"),
    JSON.stringify({ version: "0.0.14" }),
  );
  await writeFile(path.join(root, "release-assets/Ait-0.0.14-android.apk"), "universal APK");
  return root;
}

test("test release is manual, builds the captured commit, and publishes only after APK validation", () => {
  assert.deepEqual(Object.keys(workflow.on), ["workflow_dispatch"]);
  assert.equal(workflow.permissions.contents, "read");
  assert.equal(workflow.jobs.prepare.steps[0].with.ref, "${{ github.sha }}");
  assert.equal(workflow.jobs.apk.uses, "./.github/workflows/release-android.yml");
  assert.deepEqual(workflow.jobs.apk.secrets, { EXPO_TOKEN: "${{ secrets.EXPO_TOKEN }}" });
  assert.equal(workflow.jobs.apk.with.source_commit, "${{ needs.prepare.outputs.source_commit }}");
  assert.deepEqual(workflow.jobs.publish.needs, ["prepare", "apk"]);
  assert.equal(workflow.jobs.publish.permissions.contents, "write");
  for (const job of Object.values(workflow.jobs)) {
    for (const step of job.steps ?? []) {
      if (!step.run) continue;
      assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
      const result = spawnSync("bash", ["-n"], { input: step.run, encoding: "utf8" });
      assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
    }
  }
});

test("release plan uses the triggering commit and separates test tags from stable tags", async (t) => {
  const root = await fixture(t);
  const output = path.join(root, "outputs");
  const result = run(planStep.run, root, { GITHUB_SHA: commit, GITHUB_OUTPUT: output });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    await readFile(output, "utf8"),
    `tag=v0.0.14\nsource_commit=${commit}\ntest_tag=android-test-v0.0.14-${commit.slice(0, 12)}\n`,
  );
  assert.notEqual(run(planStep.run, root, { GITHUB_SHA: "main", GITHUB_OUTPUT: output }).status, 0);
});

test("test release metadata and checksums cover the universal APK and reject incomplete artifacts", async (t) => {
  const root = await fixture(t);
  const assets = path.join(root, "release-assets");
  const record = publishSteps.find((step) => step.name === "Verify assets and record test build");
  const env = {
    VERSION_TAG: "v0.0.14",
    SOURCE_COMMIT: commit,
    TEST_TAG: `android-test-v0.0.14-${commit.slice(0, 12)}`,
    GITHUB_WORKFLOW_SHA: commit,
    GITHUB_SERVER_URL: "https://github.com",
    GITHUB_REPOSITORY: "example/ait",
    GITHUB_RUN_ID: "123",
  };
  const result = run(record.run, root, env);
  assert.equal(result.status, 0, result.stderr);
  const info = JSON.parse(await readFile(path.join(assets, "BUILD-INFO.json"), "utf8"));
  assert.equal(info.sourceCommit, commit);
  assert.equal(info.releaseTag, env.TEST_TAG);
  assert.equal(info.signing, "eas-managed");
  const checksum = publishSteps.find((step) => step.name === "Write APK checksums");
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
      TEST_TAG: `android-test-v0.0.14-${commit.slice(0, 12)}`,
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
