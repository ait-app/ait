import nativeRelease from "../apps/mobile/native-release-version.js";
import { readFile, appendFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const { getNativeReleaseVersion } = nativeRelease;
const stableTagPattern = /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/;
const retryableStatuses = new Set(["errored", "canceled"]);

function normalizeStatus(status) {
  return String(status).toLowerCase().replaceAll("_", "-");
}

export function planRelease({ tag, pkgVersion }) {
  if (!stableTagPattern.test(tag))
    throw new Error(`Release tag must be a stable vX.Y.Z tag: ${tag}`);
  if (tag !== `v${pkgVersion}`) {
    throw new Error(`Release tag ${tag} differs from package version ${pkgVersion}`);
  }

  const { appVersion, iosBuildNumber } = getNativeReleaseVersion(pkgVersion);
  return { tag, appVersion, buildNumber: iosBuildNumber };
}

function isMatchingBuild(build, buildNumber) {
  return build.platform.toLowerCase() === "ios" && String(build.appBuildVersion) === buildNumber;
}

function isReleaseBuild(build, buildNumber, appVersion) {
  return (
    isMatchingBuild(build, buildNumber) &&
    build.buildProfile === "ait" &&
    build.appVersion === appVersion
  );
}

export function decideBuild({ builds, buildNumber, appVersion, buildId }) {
  const normalizedBuildNumber = String(buildNumber);

  if (buildId !== undefined) {
    const explicitBuild = builds.find((build) => build.id === buildId);
    if (!explicitBuild || !isReleaseBuild(explicitBuild, normalizedBuildNumber, appVersion)) {
      return { action: "reject", reason: "explicit-build-mismatch", buildId };
    }
    if (normalizeStatus(explicitBuild.status) !== "finished") {
      return { action: "reject", reason: "explicit-build-not-finished", buildId };
    }
    return { action: "reuse", buildId, buildNumber: normalizedBuildNumber };
  }

  const existingBuild = builds.find(
    (build) =>
      isMatchingBuild(build, normalizedBuildNumber) &&
      !retryableStatuses.has(normalizeStatus(build.status)),
  );
  if (existingBuild) {
    return { action: "fail", reason: "existing-build", buildId: existingBuild.id };
  }

  return { action: "build", buildNumber: normalizedBuildNumber };
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function main() {
  const { positionals, values } = parseArgs({
    options: {
      tag: { type: "string" },
      "package-json": { type: "string" },
      "build-number": { type: "string" },
      "app-version": { type: "string" },
      "builds-json": { type: "string" },
      "view-json": { type: "string" },
      "stdout-file": { type: "string" },
      "build-id": { type: "string" },
    },
    allowPositionals: true,
  });

  const command = positionals[0];
  if (command === "plan") {
    const packageJson = await readJson(values["package-json"]);
    const plan = planRelease({ tag: values.tag, pkgVersion: packageJson.version });
    const output = process.env.GITHUB_OUTPUT;
    if (!output) throw new Error("GITHUB_OUTPUT is required for plan");
    await appendFile(
      output,
      `app_version=${plan.appVersion}\nios_build_number=${plan.buildNumber}\n`,
    );
    return;
  }

  if (command === "check-duplicates") {
    const builds = await readJson(values["builds-json"]);
    const decision = decideBuild({ builds, buildNumber: values["build-number"] });
    if (decision.action === "fail") {
      throw new Error(`Existing EAS build blocks release: ${decision.buildId}`);
    }
    return;
  }

  if (command === "validate-build-id") {
    const build = await readJson(values["view-json"]);
    const decision = decideBuild({
      builds: [build],
      buildNumber: values["build-number"],
      appVersion: values["app-version"],
      buildId: values["build-id"],
    });
    if (decision.action !== "reuse") throw new Error(`Build cannot be reused: ${decision.reason}`);
    return;
  }

  if (command === "build-id") {
    const builds = await readJson(values["stdout-file"]);
    if (!Array.isArray(builds) || builds.length !== 1 || !builds[0]?.id) {
      throw new Error("Expected exactly one EAS build in JSON output");
    }
    process.stdout.write(`${builds[0].id}\n`);
    return;
  }

  if (command === "status") {
    const status = normalizeStatus((await readJson(values["view-json"])).status);
    if (status === "finished") return process.stdout.write("done\n");
    if (status === "errored" || status === "canceled") return process.stdout.write("failed\n");
    if (["new", "in-queue", "in-progress", "pending-cancel"].includes(status)) {
      return process.stdout.write("pending\n");
    }
    throw new Error(`Unknown EAS build status: ${status}`);
  }

  throw new Error(`Unknown command: ${command ?? ""}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
