import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { planAndroidRelease } from "./android-release.mjs";

const uuidPattern = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i;

export function planAndroidPlayRelease({ tag, version, sourceCommit, action }) {
  assert.match(sourceCommit, /^[a-f0-9]{40}$/i, "Invalid Android source commit");
  assert(["build-only", "internal-draft", "internal"].includes(action), "Invalid Play action");
  const release = planAndroidRelease(tag || `v${version}`, version);
  return {
    version: release.version,
    packageId: release.packageId,
    sourceCommit,
    buildProfile: "production-play",
    submitProfile:
      action === "build-only"
        ? null
        : action === "internal"
          ? "play-internal"
          : "play-internal-draft",
  };
}

// Only the Play runner uses remote build versions; APK and TestFlight keep local versions.
export function configureAndroidPlayBuild(sourceConfig, releaseConfig) {
  const identity = sourceConfig.build?.ait?.env;
  for (const key of ["EXPO_OWNER", "EXPO_SLUG", "EAS_PROJECT_ID"]) {
    assert.match(identity?.[key] ?? "", /^[A-Za-z0-9._-]+$/, `Invalid EAS identity: ${key}`);
    assert.equal(
      identity[key],
      releaseConfig.build.ait.env[key],
      `Unexpected EAS identity: ${key}`,
    );
  }
  const config = structuredClone(sourceConfig);
  config.cli = { ...config.cli, appVersionSource: "remote" };
  config.build["production-play"] = structuredClone(releaseConfig.build["production-play"]);
  config.submit ??= {};
  for (const profile of ["play-internal", "play-internal-draft"]) {
    config.submit[profile] = structuredClone(releaseConfig.submit[profile]);
  }
  return config;
}

export function androidPlayBuildArtifact(builds, plan, projectId) {
  assert(Array.isArray(builds) && builds.length === 1, "Expected exactly one EAS build");
  const build = builds[0];
  assert.equal(build.status, "FINISHED", "EAS Android build did not finish successfully");
  assert.equal(build.platform, "ANDROID", "Expected an Android EAS build");
  assert.equal(build.distribution, "STORE", "Expected a store build");
  assert.equal(build.buildProfile, plan.buildProfile, "Unexpected EAS build profile");
  assert.equal(build.appVersion, plan.version, "EAS app version differs from release");
  assert.equal(build.gitCommitHash, plan.sourceCommit, "EAS source commit differs from release");
  assert.equal(build.project?.id, projectId, "EAS project differs from Ait");
  assert.match(build.id ?? "", uuidPattern, "Invalid EAS build ID");
  assert.match(String(build.appBuildVersion), /^[1-9]\d*$/, "Invalid Android versionCode");
  const versionCode = Number(build.appBuildVersion);
  assert(versionCode <= 2_100_000_000, "Android versionCode exceeds the Play limit");
  const url = new URL(build.artifacts?.buildUrl);
  assert.equal(url.protocol, "https:", "EAS artifact must use HTTPS");
  assert(url.pathname.endsWith(".aab"), "Expected an Android App Bundle");
  return {
    id: build.id,
    url: url.href,
    versionCode,
    name: `Ait-${plan.version}-${versionCode}-android.aab`,
  };
}

export function validateAndroidPlayManifest(xml, plan, versionCode) {
  const manifest = xml.match(/<manifest\b([^>]*)>/);
  const application = xml.match(/<application\b([^>]*)>/);
  assert(manifest && application, "AAB manifest or application metadata is missing");
  const attributes = Object.fromEntries(
    [...manifest[1].matchAll(/([\w:]+)="([^"]*)"/g)].map((match) => [match[1], match[2]]),
  );
  assert.equal(attributes.package, plan.packageId, "AAB package differs from production Ait");
  assert.equal(
    attributes["android:versionName"],
    plan.version,
    "AAB versionName differs from release",
  );
  assert.equal(
    attributes["android:versionCode"],
    String(versionCode),
    "AAB versionCode differs from EAS build",
  );
  const debuggable = application[1].match(/\bandroid:debuggable="([^"]*)"/)?.[1] ?? "false";
  assert.equal(debuggable, "false", "Debuggable AAB cannot be released");
}

async function sha256(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

export async function collectAndroidPlayRelease({
  bundle,
  bundletool,
  plan,
  artifact,
  outputDir,
  run = execFileSync,
}) {
  const execute = (command, args) =>
    run(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  execute("java", ["-jar", bundletool, "validate", `--bundle=${bundle}`]);
  const signature = execute("jarsigner", [
    "-J-Duser.language=en",
    "-J-Duser.country=US",
    "-verify",
    "-verbose",
    "-certs",
    bundle,
  ]);
  assert(signature.includes("jar verified."), "AAB upload signature is missing or invalid");
  assert.doesNotMatch(signature, /jar contains unsigned entries/i, "AAB contains unsigned entries");
  const manifest = execute("java", ["-jar", bundletool, "dump", "manifest", `--bundle=${bundle}`]);
  validateAndroidPlayManifest(manifest, plan, artifact.versionCode);
  await mkdir(outputDir, { recursive: true });
  await copyFile(bundle, path.join(outputDir, artifact.name));
  const info = {
    version: plan.version,
    versionCode: artifact.versionCode,
    packageId: plan.packageId,
    sourceCommit: plan.sourceCommit,
    easBuildId: artifact.id,
    buildProfile: plan.buildProfile,
    submitProfile: plan.submitProfile,
    signing: "eas-managed-upload-key",
  };
  await writeFile(path.join(outputDir, "BUILD-INFO.json"), JSON.stringify(info, null, 2) + "\n");
  const sums = await Promise.all(
    [artifact.name, "BUILD-INFO.json"].map(
      async (name) => `${await sha256(path.join(outputDir, name))}  ${name}`,
    ),
  );
  await writeFile(path.join(outputDir, "SHA256SUMS"), sums.join("\n") + "\n");
  return info;
}

async function main() {
  const { values } = parseArgs({
    options: Object.fromEntries(
      ["bundle", "bundletool", "plan", "artifact", "output-dir"].map((key) => [
        key,
        { type: "string" },
      ]),
    ),
  });
  for (const key of ["bundle", "bundletool", "plan", "artifact", "output-dir"])
    assert(values[key], `Missing --${key}`);
  const info = await collectAndroidPlayRelease({
    bundle: values.bundle,
    bundletool: values.bundletool,
    plan: JSON.parse(await readFile(values.plan, "utf8")),
    artifact: JSON.parse(await readFile(values.artifact, "utf8")),
    outputDir: values["output-dir"],
  });
  console.log(JSON.stringify(info, null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
