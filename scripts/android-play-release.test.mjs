import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { EasJsonAccessor, EasJsonUtils, Platform } from "@expo/eas-json";
import yaml from "yaml";
import {
  androidPlayBuildArtifact,
  collectAndroidPlayRelease,
  configureAndroidPlayBuild,
  planAndroidPlayRelease,
  validateAndroidPlayManifest,
} from "./android-play-release.mjs";

const sourceCommit = "0123456789abcdef".repeat(2) + "01234567";
const config = JSON.parse(
  await readFile(new URL("../apps/mobile/eas.json", import.meta.url), "utf8"),
);
const plan = planAndroidPlayRelease({ version: "0.0.22", sourceCommit, action: "internal" });
const build = {
  id: "01234567-89ab-cdef-0123-456789abcdef",
  status: "FINISHED",
  platform: "ANDROID",
  distribution: "STORE",
  buildProfile: "production-play",
  appVersion: "0.0.22",
  appBuildVersion: "47",
  gitCommitHash: sourceCommit,
  project: { id: config.build.ait.env.EAS_PROJECT_ID },
  artifacts: { buildUrl: "https://expo.dev/artifacts/eas/app.aab" },
};
const artifact = androidPlayBuildArtifact([build], plan, build.project.id);
const manifest = `<manifest xmlns:android="http://schemas.android.com/apk/res/android"
  package="dev.ait.mobile" android:versionCode="47" android:versionName="0.0.22">
  <application android:label="Ait" /></manifest>`;

test("Play planning separates build-only, draft and internal release actions", () => {
  for (const [action, submitProfile] of [
    ["build-only", null],
    ["internal-draft", "play-internal-draft"],
    ["internal", "play-internal"],
  ]) {
    assert.equal(
      planAndroidPlayRelease({ version: "0.0.22", sourceCommit, action }).submitProfile,
      submitProfile,
    );
  }
  assert.equal(plan.buildProfile, "production-play");
  for (const override of [
    { action: "production" },
    { sourceCommit: "bad\nsource_commit=other" },
    { tag: "v0.0.23" },
  ])
    assert.throws(() =>
      planAndroidPlayRelease({ version: "0.0.22", sourceCommit, action: "internal", ...override }),
    );
});

test("only the Play runner switches to remote versions and can build older tagged configs", () => {
  const original = structuredClone(config);
  delete original.build["production-play"];
  delete original.submit["play-internal"];
  delete original.submit["play-internal-draft"];
  const before = structuredClone(original);
  const prepared = configureAndroidPlayBuild(original, config);
  assert.deepEqual(original, before);
  assert.equal(original.cli.appVersionSource, "local");
  assert.equal(prepared.cli.appVersionSource, "remote");
  assert.equal(prepared.build["production-play"].autoIncrement, true);
  assert.equal(prepared.build["production-play"].android.buildType, "app-bundle");
  assert.equal(prepared.build["production-play"].distribution, "store");
  assert.deepEqual(prepared.build["production-apk"], config.build["production-apk"]);
  assert.deepEqual(prepared.submit.ait, config.submit.ait);
  assert.deepEqual(prepared.submit.production, config.submit.production);
  const androidSubmit = prepared.submit["play-internal"].android;
  assert.deepEqual(androidSubmit, {
    applicationId: "dev.ait.mobile",
    track: "internal",
    releaseStatus: "completed",
  });
  assert.deepEqual(prepared.submit["play-internal-draft"], {
    extends: "play-internal",
    android: { releaseStatus: "draft" },
  });
  for (const value of ["other-project", "bad\nEXPO_TOKEN=other", ""]) {
    const wrong = structuredClone(original);
    wrong.build.ait.env.EAS_PROJECT_ID = value;
    assert.throws(() => configureAndroidPlayBuild(wrong, config), /EAS identity/);
  }
});

test("EAS resolves the Play profiles into store bundles and internal-only submissions", async () => {
  const accessor = EasJsonAccessor.fromRawString(
    JSON.stringify(configureAndroidPlayBuild(config, config)),
  );
  const profile = await EasJsonUtils.getBuildProfileAsync(
    accessor,
    Platform.ANDROID,
    "production-play",
  );
  assert.equal(profile.distribution, "store");
  assert.equal(profile.buildType, "app-bundle");
  assert.equal(profile.autoIncrement, true);
  assert.equal(profile.env.APP_VARIANT, "production");
  assert.equal(profile.env.EAS_PROJECT_ID, config.build.ait.env.EAS_PROJECT_ID);
  assert.equal(profile.env.AIT_ANDROID_HERMES_O0, "1");
  assert.equal(profile.gradleCommand, undefined);
  for (const [name, releaseStatus] of [
    ["play-internal", "completed"],
    ["play-internal-draft", "draft"],
  ]) {
    const submit = await EasJsonUtils.getSubmitProfileAsync(accessor, Platform.ANDROID, name);
    assert.equal(submit.applicationId, "dev.ait.mobile");
    assert.equal(submit.track, "internal");
    assert.equal(submit.releaseStatus, releaseStatus);
  }
});

test("only a finished AAB from the requested source and EAS project can be submitted", () => {
  assert.deepEqual(artifact, {
    id: build.id,
    url: build.artifacts.buildUrl,
    versionCode: 47,
    name: "Ait-0.0.22-47-android.aab",
  });
  for (const builds of [null, {}, [], [build, build]])
    assert.throws(() => androidPlayBuildArtifact(builds, plan, build.project.id), /exactly one/);
  for (const override of [
    { status: "ERRORED" },
    { platform: "IOS" },
    { distribution: "INTERNAL" },
    { buildProfile: "production-apk" },
    { appVersion: "0.0.23" },
    { gitCommitHash: "f".repeat(40) },
    { project: { id: "other-project" } },
    { id: "bad\nartifact_url=other" },
    { appBuildVersion: "0" },
    { appBuildVersion: "1.5" },
    { appBuildVersion: "2100000001" },
    { artifacts: {} },
    { artifacts: { buildUrl: "http://expo.dev/app.aab" } },
    { artifacts: { buildUrl: "https://expo.dev/app.apk" } },
  ])
    assert.throws(() =>
      androidPlayBuildArtifact([{ ...build, ...override }], plan, build.project.id),
    );
});

test("AAB metadata must match its validated build and must not be debuggable", () => {
  validateAndroidPlayManifest(manifest, plan, 47);
  validateAndroidPlayManifest(
    manifest.replace('android:label="Ait"', 'android:debuggable="false"'),
    plan,
    47,
  );
  for (const xml of [
    "",
    manifest.replace("dev.ait.mobile", "dev.ait.mobile.debug"),
    manifest.replace('versionName="0.0.22"', 'versionName="0.0.23"'),
    manifest.replace('versionCode="47"', 'versionCode="48"'),
    manifest.replace('android:label="Ait"', 'android:debuggable="true"'),
  ])
    assert.throws(() => validateAndroidPlayManifest(xml, plan, 47));
});

test("bundle collection verifies structure and upload signature before recording checksums", async (t) => {
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
    artifact,
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
  assert.equal(info.versionCode, 47);
  assert.equal(info.easBuildId, build.id);
  assert.equal(await readFile(path.join(outputDir, artifact.name), "utf8"), "signed app bundle");
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
    await assert.rejects(
      collectAndroidPlayRelease({
        bundle,
        bundletool: "bundletool.jar",
        plan,
        artifact,
        outputDir: path.join(root, "rejected"),
        run: () => signature,
      }),
      /signature|unsigned/,
    );
  }
  await assert.rejects(
    collectAndroidPlayRelease({
      bundle,
      bundletool: "bundletool.jar",
      plan,
      artifact,
      outputDir: path.join(root, "rejected"),
      run: () => {
        throw new Error("Invalid bundle");
      },
    }),
    /Invalid bundle/,
  );
});

test("Play workflow submits only the verified build and keeps release tooling outside its source", async () => {
  const workflow = yaml.parse(
    await readFile(
      new URL("../.github/workflows/release-android-play.yml", import.meta.url),
      "utf8",
    ),
  );
  assert.deepEqual(workflow.permissions, { contents: "read" });
  assert.equal(workflow.on.push, undefined);
  assert.equal(workflow.on.workflow_dispatch.inputs.action.default, "build-only");
  assert.deepEqual(workflow.on.workflow_dispatch.inputs.action.options, [
    "build-only",
    "internal-draft",
    "internal",
  ]);
  assert.equal(workflow.concurrency["cancel-in-progress"], false);
  const steps = workflow.jobs.release.steps;
  const position = (name) => steps.findIndex((step) => step.name === name);
  assert(position("Move tooling outside the EAS source archive") < position("Build AAB with EAS"));
  assert(position("Build AAB with EAS") < position("Validate EAS build provenance"));
  assert(
    position("Verify and collect signed AAB") <
      position("Submit this verified build to Google Play"),
  );
  const submit = steps[position("Submit this verified build to Google Play")];
  assert.equal(submit.if, "inputs.action != 'build-only'");
  assert.equal(submit.env.EAS_BUILD_ID, "${{ steps.build.outputs.build_id }}");
  assert.match(submit.run, /--id "\$EAS_BUILD_ID" --non-interactive --wait/);
  assert.doesNotMatch(submit.run, /--latest/);
  const scripts = steps.filter((step) => step.run);
  for (const step of scripts) {
    assert.doesNotMatch(step.run, /\$\{\{\s*(?:inputs|secrets)\./);
    const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
});

test("workflow scripts carry source identity and a remote counter through the exact build submission", async (t) => {
  const workflow = yaml.parse(
    await readFile(
      new URL("../.github/workflows/release-android-play.yml", import.meta.url),
      "utf8",
    ),
  );
  const steps = workflow.jobs.release.steps;
  const root = await mkdtemp(path.join(tmpdir(), "ait-play-workflow-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, "apps/mobile"), { recursive: true });
  const source = structuredClone(config);
  delete source.build["production-play"];
  delete source.submit["play-internal"];
  delete source.submit["play-internal-draft"];
  await writeFile(path.join(root, "apps/mobile/eas.json"), JSON.stringify(source));
  await writeFile(
    path.join(root, "apps/mobile/package.json"),
    JSON.stringify({ version: plan.version }),
  );
  await mkdir(path.join(root, "android-play-release-tools/apps/mobile"), { recursive: true });
  await mkdir(path.join(root, "android-play-release-tools/scripts"));
  await writeFile(
    path.join(root, "android-play-release-tools/apps/mobile/eas.json"),
    JSON.stringify(config),
  );
  const helper = new URL("./android-play-release.mjs", import.meta.url).href;
  await writeFile(
    path.join(root, "android-play-release-tools/scripts/android-play-release.mjs"),
    `export * from ${JSON.stringify(helper)};\n`,
  );
  const env = {
    ...process.env,
    RUNNER_TEMP: root,
    GITHUB_ENV: path.join(root, "env"),
    GITHUB_OUTPUT: path.join(root, "output"),
    SOURCE_COMMIT: sourceCommit,
    ACTION: "internal-draft",
    TAG: "",
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
  executeInlineNode("Configure Play build and remote versionCode");
  const configured = JSON.parse(await readFile(path.join(root, "apps/mobile/eas.json"), "utf8"));
  assert.equal(configured.cli.appVersionSource, "remote");
  assert.deepEqual(configured.build["production-apk"], config.build["production-apk"]);
  assert.match(
    await readFile(env.GITHUB_ENV, "utf8"),
    new RegExp(`EAS_PROJECT_ID=${build.project.id}\\n`),
  );
  const actualPlan = JSON.parse(await readFile(path.join(root, "android-play-plan.json"), "utf8"));
  assert.equal(actualPlan.sourceCommit, sourceCommit);
  assert.equal(actualPlan.submitProfile, "play-internal-draft");
  await writeFile(path.join(root, "android-play-build.json"), JSON.stringify([build]));
  env.EAS_PROJECT_ID = build.project.id;
  executeInlineNode("Validate EAS build provenance");
  assert.deepEqual(
    JSON.parse(await readFile(path.join(root, "android-play-artifact.json"), "utf8")),
    artifact,
  );
  const outputs = await readFile(env.GITHUB_OUTPUT, "utf8");
  assert.match(outputs, /submit_profile=play-internal-draft\n/);
  assert.match(outputs, new RegExp(`build_id=${build.id}\\n`));
  assert.match(outputs, /version_code=47\n/);
});
