import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { access, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import yaml from "yaml";
import {
  collectAndroidPlayRelease,
  configureAndroidPlayGradle,
  planAndroidPlayRelease,
  validateAndroidPlayManifest,
} from "./android-play-release.mjs";

const sourceCommit = "0123456789abcdef".repeat(2) + "01234567";
const request = {
  version: "0.0.22",
  sourceCommit,
  action: "internal",
  runNumber: "47",
  runAttempt: "1",
  versionCodeBase: "10000",
};
const plan = planAndroidPlayRelease(request);
const manifest = `<manifest xmlns:android="http://schemas.android.com/apk/res/android"
  package="dev.ait.mobile" android:versionCode="14701" android:versionName="0.0.22">
  <application android:label="Ait" /></manifest>`;
const gradle = `android {
    defaultConfig {
        applicationId "dev.ait.mobile"
        versionCode 22
        versionName "0.0.22"
    }
}`;
const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/release-android-play.yml", import.meta.url), "utf8"),
);
const steps = workflow.jobs.release.steps;

test("Play planning keeps build-only, draft and internal release actions", () => {
  for (const [action, releaseStatus] of [
    ["build-only", null],
    ["internal-draft", "draft"],
    ["internal", "completed"],
  ]) {
    assert.equal(planAndroidPlayRelease({ ...request, action }).releaseStatus, releaseStatus);
  }
  assert.equal(plan.versionCode, 14701);
  assert.equal(plan.bundleName, "Ait-0.0.22-14701-android.aab");
  for (const override of [
    { action: "production" },
    { sourceCommit: "bad\nsource_commit=other" },
    { tag: "v0.0.23" },
    { runNumber: "0" },
    { runNumber: "1.5" },
    { runNumber: "9007199254740992" },
    { runNumber: "21000000" },
    { runAttempt: "0" },
    { runAttempt: "100" },
    { versionCodeBase: "-1" },
    { versionCodeBase: "invalid\nversion_code=1" },
    { versionCodeBase: "2100000000" },
  ])
    assert.throws(() => planAndroidPlayRelease({ ...request, ...override }));
});

test("new runs and retries reserve distinct versionCodes above the migration base", () => {
  const retry = planAndroidPlayRelease({ ...request, runAttempt: "2" });
  const lastRetry = planAndroidPlayRelease({ ...request, runAttempt: "99" });
  const nextRun = planAndroidPlayRelease({ ...request, runNumber: "48" });
  assert(plan.versionCode > Number(request.versionCodeBase));
  assert(plan.versionCode < retry.versionCode);
  assert(retry.versionCode < lastRetry.versionCode);
  assert(lastRetry.versionCode < nextRun.versionCode);
  assert.equal(
    planAndroidPlayRelease({ ...request, versionCodeBase: undefined }).versionCode,
    4701,
  );
});

test("only the generated Gradle versionCode changes and unexpected templates fail closed", () => {
  const configured = configureAndroidPlayGradle(gradle, plan.versionCode);
  assert.equal(configured, gradle.replace("versionCode 22", "versionCode 14701"));
  for (const input of [
    "",
    gradle.replace("versionCode 22", "versionCode computeVersion()"),
    gradle + "\nversionCode 1",
  ])
    assert.throws(
      () => configureAndroidPlayGradle(input, plan.versionCode),
      /one Gradle versionCode/,
    );
  for (const versionCode of [0, -1, 1.5, "14701", 2_100_000_001])
    assert.throws(() => configureAndroidPlayGradle(gradle, versionCode), /Invalid/);
});

test("AAB metadata must match the planned build and must not be debuggable", () => {
  validateAndroidPlayManifest(manifest, plan, plan.versionCode);
  validateAndroidPlayManifest(
    manifest.replace('android:label="Ait"', 'android:debuggable="false"'),
    plan,
    plan.versionCode,
  );
  for (const xml of [
    "",
    manifest.replace("dev.ait.mobile", "dev.ait.mobile.debug"),
    manifest.replace('versionName="0.0.22"', 'versionName="0.0.23"'),
    manifest.replace('versionCode="14701"', 'versionCode="14702"'),
    manifest.replace('android:label="Ait"', 'android:debuggable="true"'),
  ])
    assert.throws(() => validateAndroidPlayManifest(xml, plan, plan.versionCode));
});

test("bundle collection verifies structure and signature and preserves signed bytes and checksums", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-play-release-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const bundle = path.join(root, "app.aab");
  await writeFile(bundle, "signed app bundle");
  const calls = [];
  const run = (command, args) => {
    calls.push([command, ...args]);
    return command === "jarsigner"
      ? "jar verified.\n"
      : args.includes("dump")
        ? manifest
        : "Bundle is valid\n";
  };
  const outputDir = path.join(root, "output");
  const info = await collectAndroidPlayRelease({
    bundle,
    bundletool: "bundletool.jar",
    plan,
    outputDir,
    run,
  });
  assert.deepEqual(
    calls.map((call) => call[0]),
    ["java", "jarsigner", "java"],
  );
  assert(calls[0].includes("validate"));
  assert(calls[2].includes("manifest"));
  assert.equal(info.sourceCommit, sourceCommit);
  assert.equal(info.versionCode, 14701);
  assert.equal(info.buildSystem, "github-actions-gradle");
  assert.equal(info.signing, "github-actions-upload-key");
  assert.equal(await readFile(path.join(outputDir, plan.bundleName), "utf8"), "signed app bundle");
  for (const row of (await readFile(path.join(outputDir, "SHA256SUMS"), "utf8"))
    .trim()
    .split("\n")) {
    const [hash, name] = row.split("  ");
    assert.equal(
      createHash("sha256")
        .update(await readFile(path.join(outputDir, name)))
        .digest("hex"),
      hash,
    );
  }
  for (const signature of [
    "jar is unsigned.",
    "jar verified.\nThis jar contains unsigned entries.",
  ]) {
    const rejected = path.join(root, "rejected");
    await assert.rejects(
      collectAndroidPlayRelease({
        bundle,
        bundletool: "bundletool.jar",
        plan,
        outputDir: rejected,
        run: (command) => (command === "jarsigner" ? signature : "Bundle is valid\n"),
      }),
      /signature|unsigned/,
    );
    await assert.rejects(access(rejected));
  }
  await assert.rejects(
    collectAndroidPlayRelease({
      bundle,
      bundletool: "bundletool.jar",
      plan,
      outputDir: path.join(root, "invalid"),
      run: () => {
        throw new Error("Invalid bundle");
      },
    }),
    /Invalid bundle/,
  );
});

test("Play workflow builds locally and submits only the verified AAB to internal testing", async () => {
  assert.deepEqual(workflow.permissions, { contents: "read" });
  assert.equal(workflow.on.push, undefined);
  assert.equal(workflow.on.workflow_dispatch.inputs.action.default, "build-only");
  assert.deepEqual(workflow.on.workflow_dispatch.inputs.action.options, [
    "build-only",
    "internal-draft",
    "internal",
  ]);
  assert.equal(workflow.concurrency["cancel-in-progress"], false);
  const position = (name) => steps.findIndex((step) => step.name === name);
  for (const [before, after] of [
    ["Move tooling outside the Metro source tree", "Generate Android project"],
    ["Generate Android project", "Set generated Gradle versionCode"],
    ["Set generated Gradle versionCode", "Build signed AAB with Gradle"],
    ["Build signed AAB with Gradle", "Verify and collect signed AAB"],
    ["Verify and collect signed AAB", "Submit this verified AAB to Google Play"],
  ]) {
    assert(
      position(before) >= 0 && position(before) < position(after),
      `${before} before ${after}`,
    );
  }
  const submit = steps[position("Submit this verified AAB to Google Play")];
  assert.equal(submit.if, "inputs.action != 'build-only'");
  assert.equal(submit.uses, "r0adkll/upload-google-play@v1");
  assert.equal(
    submit.with.serviceAccountJsonPlainText,
    "${{ secrets.GOOGLE_PLAY_SERVICE_ACCOUNT_JSON }}",
  );
  assert.equal(submit.with.packageName, "dev.ait.mobile");
  assert.equal(submit.with.tracks, "internal");
  assert.equal(submit.with.status, "${{ steps.plan.outputs.release_status }}");
  assert.equal(
    submit.with.releaseFiles,
    "release-assets-play/${{ steps.plan.outputs.bundle_name }}",
  );
  const text = JSON.stringify(workflow);
  assert.doesNotMatch(text, /EXPO_TOKEN|eas (?:build|submit)|EAS_BUILD_ID|expo\.dev/);
  assert.equal(steps.find((step) => step.name === "Remove upload keystore").if, "always()");
  await assert.rejects(access(new URL("../.github/workflows/build-android.yml", import.meta.url)));
  for (const step of steps.filter((step) => step.run)) {
    assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
    const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
});

test("all actions require signing, only uploads require Play credentials, and keystores stay private", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-play-credentials-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const step = steps.find((step) => step.name === "Validate release request and credentials");
  const secrets = {
    KEYSTORE_BASE64: Buffer.from("test keystore").toString("base64"),
    KEYSTORE_PASSWORD: "store",
    KEY_ALIAS: "ait",
    KEY_PASSWORD: "key",
  };
  const execute = (action, overrides = {}) =>
    spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
      cwd: root,
      encoding: "utf8",
      env: {
        PATH: process.env.PATH,
        RUNNER_TEMP: root,
        ACTION: action,
        TAG: "",
        PLAY_SERVICE_ACCOUNT_JSON: "",
        ...secrets,
        ...overrides,
      },
    });
  assert.equal(execute("build-only").status, 0);
  assert.equal(await readFile(path.join(root, "android-play.keystore"), "utf8"), "test keystore");
  assert.equal((await stat(path.join(root, "android-play.keystore"))).mode & 0o777, 0o600);
  for (const action of ["build-only", "internal-draft", "internal"]) {
    for (const name of Object.keys(secrets))
      assert.notEqual(execute(action, { [name]: "" }).status, 0);
  }
  for (const action of ["internal-draft", "internal"]) {
    assert.notEqual(execute(action).status, 0);
    assert.equal(execute(action, { PLAY_SERVICE_ACCOUNT_JSON: "{}" }).status, 0);
  }
  assert.notEqual(execute("production").status, 0);
  assert.notEqual(execute("build-only", { TAG: "v0.0.22\nmalicious" }).status, 0);
  assert.notEqual(execute("build-only", { KEYSTORE_BASE64: "%%%" }).status, 0);
});

test("workflow passes the exact source, unique version and signing arguments into the Gradle build", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-play-workflow-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, "apps/mobile/android/app"), { recursive: true });
  await writeFile(
    path.join(root, "apps/mobile/package.json"),
    JSON.stringify({ version: plan.version }),
  );
  await writeFile(path.join(root, "apps/mobile/android/app/build.gradle"), gradle);
  await mkdir(path.join(root, "android-play-release-tools/scripts"), { recursive: true });
  const helper = new URL("./android-play-release.mjs", import.meta.url).href;
  await writeFile(
    path.join(root, "android-play-release-tools/scripts/android-play-release.mjs"),
    `export * from ${JSON.stringify(helper)};\n`,
  );
  const env = {
    ...process.env,
    RUNNER_TEMP: root,
    GITHUB_OUTPUT: path.join(root, "output"),
    SOURCE_COMMIT: sourceCommit,
    ACTION: "internal-draft",
    TAG: "",
    GITHUB_RUN_NUMBER: "47",
    GITHUB_RUN_ATTEMPT: "2",
    VERSION_CODE_BASE: "10000",
  };
  const executeInlineNode = (name) => {
    const script = steps
      .find((step) => step.name === name)
      .run.match(/<<'NODE'\n([\s\S]*?)\nNODE/)[1];
    const result = spawnSync(process.execPath, ["--input-type=module"], {
      input: script,
      cwd: root,
      env,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
  };
  executeInlineNode("Plan Play build and versionCode");
  executeInlineNode("Set generated Gradle versionCode");
  const actual = JSON.parse(await readFile(path.join(root, "android-play-plan.json"), "utf8"));
  assert.equal(actual.sourceCommit, sourceCommit);
  assert.equal(actual.versionCode, 14702);
  assert.equal(actual.releaseStatus, "draft");
  assert.match(
    await readFile(path.join(root, "apps/mobile/android/app/build.gradle"), "utf8"),
    /versionCode 14702/,
  );
  assert.match(
    await readFile(env.GITHUB_OUTPUT, "utf8"),
    /bundle_name=Ait-0.0.22-14702-android.aab\n/,
  );
  await writeFile(
    path.join(root, "apps/mobile/android/gradlew"),
    `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(process.env.GRADLE_ARGS_FILE, JSON.stringify(process.argv.slice(2)));\n`,
    { mode: 0o700 },
  );
  const build = steps.find((step) => step.name === "Build signed AAB with Gradle");
  const argsFile = path.join(root, "gradle-args.json");
  const result = spawnSync("bash", ["-e", "-o", "pipefail", "-c", build.run], {
    cwd: path.join(root, build["working-directory"]),
    encoding: "utf8",
    env: {
      ...env,
      GRADLE_ARGS_FILE: argsFile,
      KEYSTORE_PASSWORD: "store $ literal",
      KEY_ALIAS: "ait upload",
      KEY_PASSWORD: "key ` literal",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  const args = JSON.parse(await readFile(argsFile, "utf8"));
  assert(args.includes(":app:bundleRelease"));
  assert(args.includes(`-Pandroid.injected.signing.store.file=${root}/android-play.keystore`));
  assert(args.includes("-Pandroid.injected.signing.store.password=store $ literal"));
  assert(args.includes("-Pandroid.injected.signing.key.alias=ait upload"));
  assert(args.includes("-Pandroid.injected.signing.key.password=key ` literal"));
});
