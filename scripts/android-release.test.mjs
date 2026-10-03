import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { runInNewContext } from "node:vm";
import yaml from "yaml";
import {
  androidBuildArtifact,
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
native-code: 'arm64-v8a' 'armeabi-v7a' 'x86' 'x86_64'
`;
const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/release-android.yml", import.meta.url), "utf8"),
);
test("plans APK names using the shared release naming rules", () => {
  assert.equal(plan.versionCode, 1002003);
  assert.equal(plan.apkName, "Ait-1.2.3-android.apk");
});

test("rejects non-stable tags, mismatched versions, and invalid native version codes", () => {
  for (const tag of ["1.2.3", "v1.2.3-beta.1", "v01.2.3", "../v1.2.3", "v1.2.3\n"])
    assert.throws(() => planAndroidRelease(tag, "1.2.3"), /Invalid release tag/);
  assert.throws(() => planAndroidRelease("v1.2.4", "1.2.3"), /differs/);
  assert.throws(() => planAndroidRelease("v1.1000.3", "1.1000.3"), /collision-free/);
  assert.throws(() => planAndroidRelease("v0.0.0", "0.0.0"), /out of range/);
});

test("accepts a universal APK and preserves the EAS signing certificate", () => {
  assert.deepEqual(validateAndroidApk(badging, certificates, plan), {
    certificateSha256: digest,
    architectures: ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"],
  });
});

test("rejects wrong package identity, version, debug builds, and missing device architectures", () => {
  for (const [metadata, expected] of [
    ["", /metadata is missing/],
    [badging.replace("dev.ait.mobile", "dev.ait.mobile.debug"), /package differs/],
    [badging.replace("1.2.3", "1.2.4"), /versionName differs/],
    [badging.replace("1002003", "1002004"), /versionCode differs/],
    [badging + "application-debuggable\n", /Debuggable/],
    [badging.replace("'arm64-v8a'", ""), /missing a required ARM architecture/],
    [badging.replace("'armeabi-v7a'", ""), /missing a required ARM architecture/],
  ])
    assert.throws(() => validateAndroidApk(metadata, certificates, plan), expected);
});

test("rejects absent or ambiguous signing certificates", () => {
  for (const value of ["", certificates.replace(digest, "invalid"), certificates + certificates])
    assert.throws(() => validateAndroidApk(badging, value, plan), /one verified/);
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

test("collects the EAS universal APK without modifying its signed bytes", async (t) => {
  const directory = await mkdtemp(path.join(tmpdir(), "ait-android-universal-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const apkBytes = Buffer.from("signed universal APK");
  await writeFile(path.join(directory, "app-release.apk"), apkBytes);
  for (const [tool, output] of [
    ["apksigner", certificates],
    ["aapt", badging],
    ["zipalign", ""],
  ]) {
    await writeFile(path.join(directory, tool), `#!/bin/sh\ncat <<'OUTPUT'\n${output}\nOUTPUT\n`, {
      mode: 0o700,
    });
  }
  const assets = await collectAndroidRelease({
    tag: "v1.2.3",
    version: "1.2.3",
    sourceDir: directory,
    buildTools: directory,
    outputDir: path.join(directory, "assets"),
  });
  assert.equal(assets.length, 1);
  assert.equal(assets[0].certificateSha256, digest);
  assert.deepEqual(await readdir(path.join(directory, "assets")), ["Ait-1.2.3-android.apk"]);
  assert.deepEqual(
    await readFile(path.join(directory, "assets", "Ait-1.2.3-android.apk")),
    apkBytes,
  );
});

test("manual modes reject invalid requests before checking out source", () => {
  const guard = workflow.jobs.prepare.steps.find(
    (step) => step.name === "Validate release request",
  ).run;
  for (const [mode, tag, ref, accepted] of [
    ["test", "", "refs/heads/feature", true],
    ["test", "v1.2.3", "refs/heads/feature", false],
    ["release", "v1.2.3", "refs/heads/main", true],
    ["release", "v1.2.3", "refs/heads/feature", false],
    ["release", "", "refs/heads/main", false],
    ["release", "v01.2.3", "refs/heads/main", false],
    ["release", "v1.2.3; exit 0", "refs/heads/main", false],
    ["other", "", "refs/heads/main", false],
  ]) {
    const result = spawnSync(
      "bash",
      ["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", guard],
      { env: { ...process.env, MODE: mode, TAG: tag, GITHUB_REF: ref } },
    );
    assert.equal(result.status === 0, accepted, JSON.stringify({ mode, tag, ref }));
  }
});

test("desktop and Android releases have independent manual entry points", async () => {
  const release = yaml.parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  assert.deepEqual(release.on.push.tags, ["v*"]);
  assert(release.on.workflow_dispatch);
  assert.equal(release.on.workflow_dispatch.inputs.build_android, undefined);
  assert.equal(release.jobs.android, undefined);
  assert.equal(release.jobs.release.needs, "build");
  for (const [desktop, cancelled, expected] of [
    ["success", false, true],
    ["failure", false, false],
    ["skipped", false, false],
    ["success", true, false],
  ]) {
    const publish = runInNewContext(release.jobs.release.if.slice(3, -2), {
      needs: { build: { result: desktop } },
      cancelled: () => cancelled,
    });
    assert.equal(publish, expected, `${desktop}, cancelled=${cancelled}`);
  }
  assert.deepEqual(Object.keys(workflow.on), ["workflow_dispatch"]);
  assert.equal(workflow.on.workflow_dispatch.inputs.mode.default, "test");
  assert.deepEqual(workflow.on.workflow_dispatch.inputs.mode.options, ["test", "release"]);
  assert.equal(workflow.permissions.contents, "read");
  assert.deepEqual(workflow.jobs.publish.needs, ["prepare", "build"]);
  assert.equal(workflow.jobs.publish.permissions.contents, "write");
  const upload = workflow.jobs.build.steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.with.name, "ait-android-apk");
  const download = release.jobs.release.steps.find((step) =>
    step.uses?.startsWith("actions/download-artifact@"),
  );
  assert.equal(download.with.pattern, "ait-*");
  assert.equal(download.with["merge-multiple"], true);
  const verify = release.jobs.release.steps.find(
    (step) => step.name === "Verify assets and write checksums",
  ).run;
  assert.match(verify, /if test -f "release-assets\/Ait-\$\{RELEASE_TAG#v\}-android\.apk"/);
  assert.match(verify, /args\+=\(--android\)/);
  for (const step of workflow.jobs.build.steps) {
    if (!step.run) continue;
    assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
    assert.doesNotMatch(step.run, /gh release|apksigner sign|ANDROID_KEYSTORE/);
    const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
});

test("EAS builds the release source before checking out release tooling", () => {
  const steps = workflow.jobs.build.steps;
  const build = steps.findIndex((step) => step.name === "Build APK with EAS");
  const tooling = steps.findIndex((step) => step.name === "Check out release tooling");
  const collection = steps.findIndex((step) => step.name === "Verify and collect APK");
  assert(build >= 0 && tooling > build && collection > tooling);
  assert.equal(steps[tooling].with.ref, "${{ github.workflow_sha }}");
  assert.equal(workflow.jobs.build["runs-on"], "ubuntu-24.04");
  assert.match(steps[build].run, /eas build --platform android --profile production-apk/);
  assert.match(steps[build].run, /--non-interactive --freeze-credentials --wait --json/);
  assert.equal(steps[build].env.EXPO_TOKEN, "${{ secrets.EXPO_TOKEN }}");
  assert.doesNotMatch(JSON.stringify(steps), /setup-gradle|expo prebuild|\.\/gradlew|ndk;|cmake;/);
});

test("only a finished EAS APK build matching the release can supply artifacts", () => {
  const build = {
    id: "01234567-89ab-cdef-0123-456789abcdef",
    status: "FINISHED",
    platform: "ANDROID",
    buildProfile: "production-apk",
    appVersion: "1.2.3",
    appBuildVersion: "1002003",
    artifacts: { buildUrl: "https://expo.dev/artifacts/eas/test.apk" },
  };
  assert.deepEqual(androidBuildArtifact([build], plan), {
    id: build.id,
    url: build.artifacts.buildUrl,
  });
  for (const builds of [[], [build, build], {}, null]) {
    assert.throws(() => androidBuildArtifact(builds, plan), /exactly one/);
  }
  for (const override of [
    { status: "ERRORED" },
    { status: "IN_PROGRESS" },
    { platform: "IOS" },
    { buildProfile: "ait" },
    { appVersion: "1.2.4" },
    { appBuildVersion: "1002004" },
    { id: "bad\nartifact_url=other" },
    { artifacts: {} },
    { artifacts: { buildUrl: "http://expo.dev/test.apk" } },
  ])
    assert.throws(() => androidBuildArtifact([{ ...build, ...override }], plan));
});
