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

export function validateAndroidPlayOptimization(gradle, properties) {
  const values = new Map(
    properties.split(/\r?\n/).flatMap((line) => {
      const match = line.match(/^\s*([^#!\s=]+)\s*=\s*(.*?)\s*$/);
      return match ? [[match[1], match[2]]] : [];
    }),
  );
  for (const key of [
    "android.enableMinifyInReleaseBuilds",
    "android.enableShrinkResourcesInReleaseBuilds",
  ]) {
    assert.equal(values.get(key), "true", `Play release requires ${key}=true`);
  }
  assert.notEqual(values.get("android.enableR8.fullMode"), "false", "R8 full mode is disabled");
  assert.match(
    gradle,
    /getDefaultProguardFile\((["'])proguard-android-optimize\.txt\1\)/,
    "Play release requires optimized ProGuard defaults",
  );
  assert.doesNotMatch(gradle, /["']proguard-android\.txt["']/);
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
    run(command, args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      // R8 mappings for React Native and its Android dependencies can exceed Node's default.
      maxBuffer: 64 * 1024 * 1024,
    }).trim();
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
  const entries = execute("unzip", ["-Z1", bundle]).split(/\r?\n/);
  const mappingEntry = "BUNDLE-METADATA/com.android.tools.build.obfuscation/proguard.map";
  assert(entries.includes(mappingEntry), "AAB is missing its embedded R8 mapping");
  const mapping = execute("unzip", ["-p", bundle, mappingEntry]);
  assert.match(mapping, /^# compiler: R8\r?$/m, "AAB mapping was not generated by R8");
  assert.match(mapping, /^\S+ -> \S+:\r?$/m, "AAB R8 mapping contains no classes");
  const r8Entry = "BUNDLE-METADATA/com.android.tools/r8.json";
  const r8Metadata = entries.includes(r8Entry) ? execute("unzip", ["-p", bundle, r8Entry]) : null;
  // Older AGP versions omit r8.json; Play can still read the embedded mapping.
  if (r8Metadata !== null) JSON.parse(r8Metadata);
  await mkdir(outputDir, { recursive: true });
  await copyFile(bundle, path.join(outputDir, plan.bundleName));
  await writeFile(path.join(outputDir, "mapping.txt"), mapping + "\n");
  if (r8Metadata !== null) await writeFile(path.join(outputDir, "r8.json"), r8Metadata + "\n");
  const info = {
    version: plan.version,
    versionCode: plan.versionCode,
    packageId: plan.packageId,
    sourceCommit: plan.sourceCommit,
    buildSystem: "github-actions-gradle",
    releaseStatus: plan.releaseStatus,
    signing: "github-actions-upload-key",
    optimization: { mapping: "mapping.txt", r8Metadata: r8Metadata === null ? null : "r8.json" },
  };
  await writeFile(path.join(outputDir, "BUILD-INFO.json"), JSON.stringify(info, null, 2) + "\n");
  const sums = await Promise.all(
    [
      plan.bundleName,
      "BUILD-INFO.json",
      "mapping.txt",
      ...(r8Metadata === null ? [] : ["r8.json"]),
    ].map(async (name) => `${await sha256(path.join(outputDir, name))}  ${name}`),
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
