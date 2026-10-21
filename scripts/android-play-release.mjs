import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { planAndroidRelease } from "./android-release.mjs";

export function planAndroidPlayRelease({
  tag,
  version,
  sourceCommit,
  action,
  runNumber,
  runAttempt,
  versionCodeBase = "0",
}) {
  assert.match(sourceCommit, /^[a-f0-9]{40}$/i, "Invalid Android source commit");
  assert(["build-only", "internal-draft", "internal"].includes(action), "Invalid Play action");
  const release = planAndroidRelease(tag || `v${version}`, version);
  for (const [name, value] of Object.entries({ runNumber, runAttempt, versionCodeBase })) {
    assert.match(String(value), /^(0|[1-9]\d*)$/, `Invalid ${name}`);
    assert(Number.isSafeInteger(Number(value)), `Invalid ${name}`);
  }
  assert(Number(runNumber) > 0, "runNumber must be positive");
  assert(Number(runAttempt) > 0 && Number(runAttempt) < 100, "runAttempt must be 1 through 99");
  // Reserve 100 slots per run so retries never reuse an uploaded versionCode.
  const versionCode = Number(versionCodeBase) + Number(runNumber) * 100 + Number(runAttempt);
  assert(
    Number.isSafeInteger(versionCode) && versionCode <= 2_100_000_000,
    "Android versionCode exceeds the Play limit",
  );
  return {
    version: release.version,
    versionCode,
    packageId: release.packageId,
    sourceCommit,
    releaseStatus: action === "build-only" ? null : action === "internal" ? "completed" : "draft",
    bundleName: `Ait-${release.version}-${versionCode}-android.aab`,
  };
}

export function configureAndroidPlayGradle(gradle, versionCode) {
  assert(
    Number.isSafeInteger(versionCode) && versionCode > 0 && versionCode <= 2_100_000_000,
    "Invalid Android versionCode",
  );
  const pattern = /^(\s*versionCode\s+)\d+(\s*)$/gm;
  assert.equal([...gradle.matchAll(pattern)].length, 1, "Expected one Gradle versionCode");
  return gradle.replace(pattern, (_, prefix, suffix) => `${prefix}${versionCode}${suffix}`);
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
    "AAB versionCode differs from build plan",
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
  validateAndroidPlayManifest(manifest, plan, plan.versionCode);
  await mkdir(outputDir, { recursive: true });
  await copyFile(bundle, path.join(outputDir, plan.bundleName));
  const info = {
    version: plan.version,
    versionCode: plan.versionCode,
    packageId: plan.packageId,
    sourceCommit: plan.sourceCommit,
    buildSystem: "github-actions-gradle",
    releaseStatus: plan.releaseStatus,
    signing: "github-actions-upload-key",
  };
  await writeFile(path.join(outputDir, "BUILD-INFO.json"), JSON.stringify(info, null, 2) + "\n");
  const sums = await Promise.all(
    [plan.bundleName, "BUILD-INFO.json"].map(
      async (name) => `${await sha256(path.join(outputDir, name))}  ${name}`,
    ),
  );
  await writeFile(path.join(outputDir, "SHA256SUMS"), sums.join("\n") + "\n");
  return info;
}

async function main() {
  const { values } = parseArgs({
    options: Object.fromEntries(
      ["bundle", "bundletool", "plan", "output-dir"].map((key) => [key, { type: "string" }]),
    ),
  });
  for (const key of ["bundle", "bundletool", "plan", "output-dir"])
    assert(values[key], `Missing --${key}`);
  const info = await collectAndroidPlayRelease({
    bundle: values.bundle,
    bundletool: values.bundletool,
    plan: JSON.parse(await readFile(values.plan, "utf8")),
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
