import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { runInNewContext } from "node:vm";
import yaml from "yaml";
import {
  collectAndroidRelease,
  planAndroidRelease,
  validateAndroidApk,
} from "./android-release.mjs";

const plan = planAndroidRelease("v1.2.3", "1.2.3");
const digest = "ab".repeat(32);
const certificates = `Signer #1 certificate DN: CN=Ait Release
Signer #1 certificate SHA-256 digest: ${digest}
Signer #1 public key SHA-256 digest: ${"cd".repeat(32)}
`;
const badging = `package: name='dev.ait.mobile' versionCode='1002003' versionName='1.2.3'
sdkVersion:'29'
native-code: 'arm64-v8a'
`;
const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/release-android.yml", import.meta.url), "utf8"),
);

test("plans APK names using the shared release naming rules", () => {
  assert.equal(plan.versionCode, 1002003);
  assert.deepEqual(plan.apks, [
    { name: "Ait-1.2.3-android-arm64.apk", abi: "arm64-v8a" },
    { name: "Ait-1.2.3-android-armv7.apk", abi: "armeabi-v7a" },
  ]);
});

test("rejects non-stable tags, mismatched versions, and invalid native version codes", () => {
  for (const tag of ["1.2.3", "v1.2.3-beta.1", "v01.2.3", "../v1.2.3", "v1.2.3\n"])
    assert.throws(() => planAndroidRelease(tag, "1.2.3"), /Invalid release tag/);
  assert.throws(() => planAndroidRelease("v1.2.4", "1.2.3"), /differs/);
  assert.throws(() => planAndroidRelease("v1.1000.3", "1.1000.3"), /collision-free/);
  assert.throws(() => planAndroidRelease("v0.0.0", "0.0.0"), /out of range/);
});

test("accepts each release architecture and preserves the generated debug certificate", () => {
  for (const abi of ["arm64-v8a", "armeabi-v7a"]) {
    assert.deepEqual(
      validateAndroidApk(
        badging.replace("arm64-v8a", abi),
        certificates.replace("Ait Release", "Android Debug"),
        plan,
        abi,
      ),
      { certificateSha256: digest, architectures: [abi] },
    );
  }
});

test("rejects wrong package identity, version, debug builds, and missing device architectures", () => {
  for (const [metadata, expected] of [
    ["", /metadata is missing/],
    [badging.replace("dev.ait.mobile", "dev.ait.mobile.debug"), /package differs/],
    [badging.replace("1.2.3", "1.2.4"), /versionName differs/],
    [badging.replace("1002003", "1002004"), /versionCode differs/],
    [badging + "application-debuggable\n", /Debuggable/],
    [badging.replace("'arm64-v8a'", ""), /architecture differs/],
    [badging.replace("'arm64-v8a'", "'armeabi-v7a'"), /architecture differs/],
    [badging.replace("'arm64-v8a'", "'arm64-v8a' 'armeabi-v7a'"), /architecture differs/],
  ])
    assert.throws(() => validateAndroidApk(metadata, certificates, plan, "arm64-v8a"), expected);
});

test("rejects absent or ambiguous signing certificates", () => {
  for (const value of ["", certificates.replace(digest, "invalid"), certificates + certificates])
    assert.throws(() => validateAndroidApk(badging, value, plan, "arm64-v8a"), /one verified/);
});

test("failed APK verification produces no release assets and needs no signing secrets", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "ait-android-test-"));
  try {
    await writeFile(path.join(directory, "apksigner"), "#!/bin/sh\nexit 7\n", { mode: 0o700 });
    await assert.rejects(
      collectAndroidRelease({
        tag: "v1.2.3",
        version: "1.2.3",
        sourceDir: directory,
        buildTools: directory,
        outputDir: path.join(directory, "assets"),
      }),
      /Command failed/,
    );
    assert.deepEqual(await readdir(directory), ["apksigner"]);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("workflow input guard accepts stable releases and fails closed on malformed refs", () => {
  const guard = workflow.jobs.build.steps.find(
    (step) => step.name === "Validate release inputs",
  ).run;
  for (const [tag, source, accepted] of [
    ["v1.2.3", "", true],
    ["v1.2.3", "a".repeat(40), true],
    ["v1.2.3-beta.1", "", false],
    ["v01.2.3", "", false],
    ["v1.2.3\n", "", false],
    ["v1.2.3", "main", false],
    ["v1.2.3", "a".repeat(39), false],
    ["v1.2.3; exit 0", "", false],
  ]) {
    const result = spawnSync(
      "bash",
      ["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", guard],
      {
        env: { ...process.env, RELEASE_TAG: tag, SOURCE_COMMIT: source },
      },
    );
    assert.equal(result.status === 0, accepted, JSON.stringify({ tag, source }));
  }
});

test("the normal release builds Android only when explicitly selected", async () => {
  const release = yaml.parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  assert.deepEqual(release.on.push.tags, ["v*"]);
  assert(release.on.workflow_dispatch);
  assert.equal(release.on.workflow_dispatch.inputs.build_android.type, "boolean");
  assert.equal(release.on.workflow_dispatch.inputs.build_android.default, false);
  for (const [event, selected, expected] of [
    ["push", undefined, false],
    ["push", true, false],
    ["workflow_dispatch", undefined, false],
    ["workflow_dispatch", false, false],
    ["workflow_dispatch", true, true],
  ]) {
    const enabled = runInNewContext(release.jobs.android.if, {
      github: { event_name: event },
      inputs: { build_android: selected },
    });
    assert.equal(enabled, expected, `${event}, Android=${selected}`);
  }
  assert.equal(release.jobs.android.uses, "./.github/workflows/release-android.yml");
  assert.deepEqual(release.jobs.android.with, {
    tag: "${{ inputs.tag || github.ref_name }}",
    source_commit: "${{ inputs.source_commit || '' }}",
  });
  assert.deepEqual(release.jobs.release.needs, ["build", "android"]);
  for (const [selected, desktop, android, cancelled, expected] of [
    [undefined, "success", "skipped", false, true],
    [false, "success", "skipped", false, true],
    [true, "success", "success", false, true],
    [true, "success", "failure", false, false],
    [true, "success", "skipped", false, false],
    [true, "success", "cancelled", false, false],
    [false, "failure", "skipped", false, false],
    [false, "skipped", "skipped", false, false],
    [false, "success", "skipped", true, false],
  ]) {
    const publish = runInNewContext(release.jobs.release.if.slice(3, -2), {
      inputs: { build_android: selected },
      needs: { build: { result: desktop }, android: { result: android } },
      cancelled: () => cancelled,
    });
    assert.equal(publish, expected, `${selected}, ${desktop}, ${android}, cancelled=${cancelled}`);
  }
  assert.equal(release.jobs.release.env.BUILD_ANDROID, "${{ inputs.build_android || false }}");
  assert.deepEqual(Object.keys(workflow.on), ["workflow_call"]);
  assert.equal(workflow.permissions.contents, "read");
  assert.equal(workflow.on.workflow_call.secrets, undefined);
  assert.equal(workflow.jobs.publish, undefined);
  assert.equal(release.jobs.android.secrets, undefined);
  const upload = workflow.jobs.build.steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.with.name, "ait-android-apk");
  const download = release.jobs.release.steps.find((step) =>
    step.uses?.startsWith("actions/download-artifact@"),
  );
  assert.equal(download.with.pattern, "ait-*");
  assert.equal(download.with["merge-multiple"], true);
  for (const step of workflow.jobs.build.steps) {
    if (!step.run) continue;
    assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
    assert.doesNotMatch(step.run, /gh release|apksigner sign|ANDROID_KEYSTORE/);
    const result = spawnSync("bash", ["-n"], { input: step.run, encoding: "utf8" });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
});

test("release tooling is checked out after Metro finishes to avoid duplicate workspace packages", () => {
  const steps = workflow.jobs.build.steps;
  const build = steps.findIndex((step) => step.name === "Assemble release APKs");
  const tooling = steps.findIndex((step) => step.name === "Check out release tooling");
  const collection = steps.findIndex((step) => step.name === "Verify and collect APKs");
  assert(build >= 0 && tooling > build && collection > tooling);
  assert.equal(steps[tooling].with.ref, "${{ github.workflow_sha }}");
});
