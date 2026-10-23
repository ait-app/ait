import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { parse } from "yaml";

const workflow = parse(
  readFileSync(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
);

test("release publication waits for the reusable Windows build and requires its assets", () => {
  assert.equal(workflow.jobs.windows.uses, "./.github/workflows/windows-desktop.yml");
  assert.equal(
    workflow.jobs.windows.with.source_ref,
    "${{ inputs.source_commit || inputs.tag || github.ref }}",
  );
  assert.deepEqual(workflow.jobs.release.needs, ["build", "windows"]);
  assert.match(workflow.jobs.release.if, /needs\.windows\.result == 'success'/);
  const verify = workflow.jobs.release.steps.find(
    (step) => step.name === "Verify assets and write checksums",
  );
  assert.match(verify.run, /--windows/);
});

test("Windows packaging only runs as a reusable release workflow and smokes the native client", () => {
  const windows = parse(
    readFileSync(new URL("../.github/workflows/windows-desktop.yml", import.meta.url), "utf8"),
  );
  assert.deepEqual(Object.keys(windows.on), ["workflow_call"]);
  assert.equal(windows.permissions.contents, "read");
  const job = windows.jobs.build;
  assert.equal(job["runs-on"], "windows-2025");
  const commands = job.steps.map((step) => step.run ?? "").join("\n");
  assert.match(commands, /npm run build:windows/);
  assert.match(commands, /windows-startup\.e2e\.mjs/);
  assert.match(commands, /release-assets\.mjs collect win/);
  assert.doesNotMatch(commands, /cargo|prepare-daemon|gh release/);
  assert(!job.steps.some((step) => step.uses?.includes("rust-toolchain")));
  assert.equal(
    job.steps.find((step) => step.uses?.startsWith("actions/upload-artifact")).with.name,
    "ait-windows-x64",
  );
});

function runStep(t, name, state, tag = "v0.1.1", builtAssets = []) {
  const step = workflow.jobs.release.steps.find((candidate) => candidate.name === name);
  assert(step, `Missing release step: ${name}`);
  const directory = mkdtempSync(path.join(tmpdir(), "ait-release-workflow-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const stateFile = path.join(directory, "state.json");
  const logFile = path.join(directory, "calls.jsonl");
  writeFileSync(stateFile, JSON.stringify({ tagExists: true, assets: [], ...state }));
  writeFileSync(logFile, "");
  mkdirSync(path.join(directory, "release-assets"));
  writeFileSync(path.join(directory, "release-assets", "artifact.bin"), "installer");
  for (const name of builtAssets)
    writeFileSync(path.join(directory, "release-assets", name), "apk");
  const executable = path.join(directory, "gh");
  writeFileSync(
    executable,
    `#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);
fs.appendFileSync(process.env.AIT_RELEASE_TEST_LOG, JSON.stringify(args) + "\\n");
const file = process.env.AIT_RELEASE_TEST_STATE;
const state = JSON.parse(fs.readFileSync(file, "utf8"));
if (args[0] !== "release") process.exit(9);
switch (args[1]) {
  case "view":
    if (!state.release) process.exit(1);
    if (args.includes("--json")) console.log(state.assets.length);
    process.exit(0);
  case "create":
    if (state.release) process.exit(2);
    if (args.includes("--verify-tag") && !state.tagExists) process.exit(3);
    state.release = { draft: args.includes("--draft"), prerelease: args.includes("--prerelease") };
    state.assets = ["artifact.bin"];
    break;
  case "edit":
    if (!state.release) process.exit(4);
    if (args.includes("--draft=false")) state.release.draft = false;
    if (args.includes("--draft")) state.release.draft = true;
    break;
  case "upload":
    if (!state.release) process.exit(5);
    state.uploaded = true;
    break;
  case "download":
    if (state.assets.length === 0) process.exit(6);
    break;
  default:
    process.exit(7);
}
fs.writeFileSync(file, JSON.stringify(state));
`,
  );
  chmodSync(executable, 0o755);
  const result = spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
    cwd: directory,
    encoding: "utf8",
    env: {
      PATH: `${directory}${path.delimiter}${process.env.PATH}`,
      RELEASE_TAG: tag,
      GITHUB_REPOSITORY: "ait-app/ait",
      AIT_RELEASE_TEST_STATE: stateFile,
      AIT_RELEASE_TEST_LOG: logFile,
    },
  });
  return {
    result,
    state: JSON.parse(readFileSync(stateFile, "utf8")),
    calls: readFileSync(logFile, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse),
  };
}

const publishStep = "Create or repair GitHub Release";

test("a new stable release stops as a draft with uploaded installers", (t) => {
  const { result, state } = runStep(t, publishStep, { release: null });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(state.release, { draft: true, prerelease: false });
  assert.deepEqual(state.assets, ["artifact.bin"]);
});

test("a new beta remains a draft and cannot become the latest stable release", (t) => {
  const { result, state, calls } = runStep(t, publishStep, { release: null }, "v0.1.1-beta.1");
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(state.release, { draft: true, prerelease: true });
  assert(calls.find((args) => args[1] === "create").includes("--latest=false"));
});

test("rerunning asset upload keeps an existing release draft awaiting review", (t) => {
  const { result, state, calls } = runStep(t, publishStep, { release: { draft: true } });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(state.release.draft, true);
  assert.equal(state.uploaded, true);
  assert(!calls.some((args) => args[1] === "create"));
});

test("repairing an existing published release preserves its publication state", (t) => {
  const { result, state } = runStep(t, publishStep, { release: { draft: false } });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(state.release.draft, false);
  assert.equal(state.uploaded, true);
});

test("release creation fails before uploading when the verified tag is absent", (t) => {
  const { result, state } = runStep(t, publishStep, { release: null, tagExists: false });
  assert.notEqual(result.status, 0);
  assert.equal(state.release, null);
});

test("an empty draft can be retried without attempting to download missing assets", (t) => {
  const { result, calls } = runStep(t, "Preserve previously published Android APK", {
    release: { draft: true },
  });
  assert.equal(result.status, 0, result.stderr);
  assert(!calls.some((args) => args[1] === "download"));
});

test("a freshly built APK is published instead of the previous one", (t) => {
  const apk = "Ait-0.1.1-android.apk";
  const { result, calls } = runStep(
    t,
    "Preserve previously published Android APK",
    { release: { draft: false }, assets: [apk, "SHA256SUMS"] },
    "v0.1.1",
    [apk],
  );
  assert.equal(result.status, 0, result.stderr);
  assert(!calls.some((args) => args[1] === "download"));
});
