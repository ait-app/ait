import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, readFile } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { releaseAssetNames } from "./release-assets.mjs";
import nativeRelease from "../apps/mobile/native-release-version.js";

export function planAndroidRelease(tag, version) {
  assert.match(tag, /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/, "Invalid release tag");
  assert.equal(tag, tag.trim(), "Invalid release tag");
  assert.equal(tag, `v${version}`, "Release tag differs from package version");
  const { appVersion, androidVersionCode } = nativeRelease.getNativeReleaseVersion(version);
  return {
    version: appVersion,
    versionCode: androidVersionCode,
    packageId: "dev.ait.mobile",
    apkName: releaseAssetNames("android", version)[0],
  };
}

export function androidBuildArtifact(builds, plan) {
  assert(Array.isArray(builds) && builds.length === 1, "Expected exactly one EAS build");
  const build = builds[0];
  assert.equal(build.status, "FINISHED", "EAS Android build did not finish successfully");
  assert.equal(build.platform, "ANDROID", "Expected an Android EAS build");
  assert.equal(build.buildProfile, "production-apk", "Unexpected EAS build profile");
  assert.equal(build.appVersion, plan.version, "EAS app version differs from release");
  assert.equal(
    String(build.appBuildVersion),
    String(plan.versionCode),
    "EAS versionCode differs from release",
  );
  assert.match(build.id ?? "", /^[a-f0-9-]{36}$/i, "Invalid EAS build ID");
  const url = new URL(build.artifacts?.buildUrl);
  assert.equal(url.protocol, "https:", "EAS artifact must use HTTPS");
  return { id: build.id, url: url.href };
}

export function validateAndroidApk(badging, certificates, plan) {
  const packageLine = badging.split("\n").find((line) => line.startsWith("package:"));
  assert(packageLine, "APK package metadata is missing");
  const attributes = Object.fromEntries(
    [...packageLine.matchAll(/(\w+)='([^']*)'/g)].map((match) => [match[1], match[2]]),
  );
  assert.equal(attributes.name, plan.packageId, "APK package differs from production Ait");
  assert.equal(attributes.versionName, plan.version, "APK versionName differs from release");
  assert.equal(
    attributes.versionCode,
    String(plan.versionCode),
    "APK versionCode differs from release",
  );
  assert(!/^application-debuggable\b/m.test(badging), "Debuggable APK cannot be released");
  const nativeCode = badging.split("\n").find((line) => line.startsWith("native-code:")) ?? "";
  const architectures = [...nativeCode.matchAll(/'([^']+)'/g)].map((match) => match[1]);
  assert(
    ["arm64-v8a", "armeabi-v7a"].every((abi) => architectures.includes(abi)),
    "Universal APK is missing a required ARM architecture",
  );
  const signers = [
    ...certificates.matchAll(/^Signer #\d+ certificate SHA-256 digest: ([\da-f]{64})\s*$/gim),
  ];
  assert.equal(signers.length, 1, "Expected one verified Android signing certificate");
  return { certificateSha256: signers[0][1].toLowerCase(), architectures };
}

export async function collectAndroidRelease({ tag, version, sourceDir, buildTools, outputDir }) {
  const plan = planAndroidRelease(tag, version);
  const run = (name, args) =>
    execFileSync(path.join(buildTools, name), args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  const apk = path.join(sourceDir, "app-release.apk");
  const certificates = run("apksigner", ["verify", "--print-certs", apk]);
  run("zipalign", ["-c", "-P", "16", "4", apk]);
  const verified = validateAndroidApk(run("aapt", ["dump", "badging", apk]), certificates, plan);
  await mkdir(outputDir, { recursive: true });
  await copyFile(apk, path.join(outputDir, plan.apkName));
  return [{ name: plan.apkName, apk, ...verified }];
}

async function main() {
  const { values } = parseArgs({
    options: {
      tag: { type: "string" },
      "package-json": { type: "string" },
      "source-dir": { type: "string" },
      "build-tools": { type: "string" },
      "output-dir": { type: "string" },
    },
  });
  for (const name of ["tag", "package-json", "source-dir", "build-tools", "output-dir"])
    assert(values[name], `Missing --${name}`);
  const pkg = JSON.parse(await readFile(values["package-json"], "utf8"));
  const info = await collectAndroidRelease({
    tag: values.tag,
    version: pkg.version,
    sourceDir: values["source-dir"],
    buildTools: values["build-tools"],
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
