import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { devDisplayVersion, devPackagedVersion } from "./dev-build.mjs";
import { releaseChannel } from "./release-version.mjs";
import { resolveReleaseLayout } from "./release-layout.mjs";

export function releaseAssetNames(platform, version, buildLabel) {
  const channel = buildLabel ? "latest" : releaseChannel(version);
  if (buildLabel !== undefined)
    assert.match(buildLabel, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid dev build label");
  const label = buildLabel ?? version;
  if (platform === "linux")
    return [
      buildLabel ? `Ait-${label}-linux-x86_64.AppImage` : `Ait-linux-x86_64.AppImage`,
      `Ait-${label}-linux-x64.tar.gz`,
      `Ait-${label}-linux-amd64.deb`,
      `Ait-${label}-linux-x86_64.rpm`,
      `${channel}-linux.yml`,
    ];
  if (platform === "mac")
    return [`Ait-${label}-macos-arm64.dmg`, `Ait-${label}-macos-arm64.zip`, `${channel}-mac.yml`];
  if (platform === "win" && !buildLabel)
    return [
      `Ait-${version}-windows-x64-setup.exe`,
      `Ait-${version}-windows-x64.zip`,
      `${channel}.yml`,
    ];
  if (platform === "android" && !buildLabel) return [`Ait-${version}-android.apk`];
  throw new Error(`Unsupported release platform: ${platform}`);
}

// Standalone headless daemon archives; the target suffix keeps them apart from desktop tarballs.
const daemonTargets = { linux: "linux-x64", mac: "macos-arm64" };
export const daemonExecutable = "aitd";

// Dev build archives carry the build label, like the dev desktop packages.
export function daemonAssetName(platform, version, buildLabel) {
  releaseChannel(version);
  if (buildLabel !== undefined)
    assert.match(buildLabel, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid dev build label");
  assert(daemonTargets[platform], `Unsupported daemon platform: ${platform}`);
  return `Ait-${buildLabel ?? version}-daemon-${daemonTargets[platform]}.tar.gz`;
}

export const daemonAssetNames = (version, buildLabel) =>
  Object.keys(daemonTargets).map((platform) => daemonAssetName(platform, version, buildLabel));

export async function packDaemonAsset({
  platform,
  version,
  binary,
  bridge = path.join(path.dirname(binary), "ait-omp-acp"),
  sourceLayout,
  destination,
  license = "LICENSE",
  buildLabel,
}) {
  const name = daemonAssetName(platform, version, buildLabel);
  // Old immutable tags name the binary `server`; only the reported version must match.
  const reported = execFileSync(binary, ["--version"], {
    encoding: "utf8",
  }).trim();
  assert.equal(
    reported.split(/\s+/).at(-1),
    buildLabel ? devDisplayVersion(buildLabel) : version,
    `Daemon binary reports ${reported}`,
  );
  const includeBridge = sourceLayout ? resolveReleaseLayout(sourceLayout).ompBridge : true;
  if (includeBridge) {
    const reportedBridge = execFileSync(bridge, ["--version"], { encoding: "utf8" }).trim();
    assert.equal(
      reportedBridge,
      `ait-omp-acp ${buildLabel ? devDisplayVersion(buildLabel) : version}`,
      `OMP ACP bridge reports ${reportedBridge}`,
    );
  }
  const staging = await mkdtemp(path.join(tmpdir(), "ait-daemon-"));
  try {
    await copyFile(binary, path.join(staging, daemonExecutable));
    await chmod(path.join(staging, daemonExecutable), 0o755);
    const files = [daemonExecutable];
    if (includeBridge) {
      await copyFile(bridge, path.join(staging, "ait-omp-acp"));
      await chmod(path.join(staging, "ait-omp-acp"), 0o755);
      files.push("ait-omp-acp");
    }
    await copyFile(license, path.join(staging, "LICENSE"));
    files.push("LICENSE");
    await mkdir(destination, { recursive: true });
    execFileSync("tar", ["-czf", path.resolve(destination, name), "-C", staging, ...files]);
    verifyDaemonArchive(path.resolve(destination, name), includeBridge);
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
  return name;
}

export function verifyDaemonArchive(archive, includeBridge = true) {
  const expected = ["aitd", ...(includeBridge ? ["ait-omp-acp"] : []), "LICENSE"];
  const entries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" }).trim().split("\n");
  assert.deepEqual(entries.sort(), expected.sort(), "Unexpected standalone archive contents");
  const modes = execFileSync("tar", ["-tvzf", archive], { encoding: "utf8" }).trim().split("\n");
  for (const entry of modes) {
    const name = entry.split(/\s+/).at(-1);
    assert(entry.startsWith("-"), `Archive member must be a regular file: ${name}`);
    if (name !== "LICENSE")
      assert.match(entry, /^-[r-][w-]x/, `Archive binary is not executable: ${name}`);
  }
}

async function digest(file, algorithm, encoding) {
  const hash = createHash(algorithm);
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest(encoding);
}

export async function collectReleaseAssets({ platform, version, source, destination, buildLabel }) {
  const names = releaseAssetNames(platform, version, buildLabel);
  // Updater metadata is listed last; every installer before it may appear in its file list.
  const installers = names.slice(0, -1);
  for (const name of names)
    assert((await stat(path.join(source, name))).size > 0, `Empty release asset: ${name}`);
  const { parse } = await import("yaml");
  const metadata = parse(await readFile(path.join(source, names.at(-1)), "utf8"));
  assert.equal(
    metadata.version,
    buildLabel ? devPackagedVersion(buildLabel) : version,
    "Updater metadata version differs from release",
  );
  assert(
    Array.isArray(metadata.files) && metadata.files.length > 0,
    "Updater file list is missing",
  );
  const updateFile = platform === "mac" ? names[1] : names[0];
  assert(
    metadata.files.some((file) => file.url === updateFile),
    `Updater metadata must reference ${updateFile}`,
  );
  for (const file of metadata.files) {
    assert(installers.includes(file.url), `Unexpected updater asset: ${file.url}`);
    const sourcePath = path.join(source, file.url);
    assert.equal(
      await digest(sourcePath, "sha512", "base64"),
      file.sha512,
      `Updater checksum mismatch: ${file.url}`,
    );
    if (file.size !== undefined)
      assert.equal((await stat(sourcePath)).size, file.size, `Updater size mismatch: ${file.url}`);
  }
  if (metadata.path !== undefined) {
    assert(installers.includes(metadata.path), `Unexpected updater path: ${metadata.path}`);
    assert.equal(
      await digest(path.join(source, metadata.path), "sha512", "base64"),
      metadata.sha512,
      "Legacy updater checksum mismatch",
    );
  }
  const existing = new Set(await readdir(source));
  const blockmaps = installers
    .map((name) => `${name}.blockmap`)
    .filter((name) => existing.has(name));
  await mkdir(destination, { recursive: true });
  for (const name of [...names, ...blockmaps])
    await copyFile(path.join(source, name), path.join(destination, name));
  return [...names, ...blockmaps];
}

export async function verifyReleaseAssets({
  version,
  directory,
  includeAndroid = false,
  includeDaemon = false,
  includeWindows = false,
  buildLabel,
  sourceLayout,
}) {
  assert(!(includeAndroid && buildLabel), "Dev builds support desktop platforms only");
  assert(!(includeWindows && buildLabel), "Windows dev builds are not configured");
  const names = (await readdir(directory)).filter((name) => name !== "SHA256SUMS").sort();
  const windows = buildLabel ? [] : releaseAssetNames("win", version);
  const windowsBlockmaps = windows
    .filter((name) => !name.endsWith(".yml"))
    .map((name) => `${name}.blockmap`);
  const windowsPresent = names.some((name) =>
    [...windows, ...windowsBlockmaps, "BUILD-INFO-windows.json"].includes(name),
  );
  const desktop = [
    ...releaseAssetNames("linux", version, buildLabel),
    ...releaseAssetNames("mac", version, buildLabel),
    ...(includeWindows || windowsPresent ? windows : []),
  ];
  const required = [
    ...desktop,
    ...(includeAndroid ? releaseAssetNames("android", version) : []),
    // Dev builds always carry both daemons; releases opt in, so Android repairs can skip them.
    ...(includeDaemon || buildLabel ? daemonAssetNames(version, buildLabel) : []),
  ];
  // Android repairs re-verify stable releases that may or may not carry daemon archives.
  const allowed = new Set([
    ...required,
    ...(buildLabel ? [] : daemonAssetNames(version)),
    "BUILD-INFO.json",
    ...(buildLabel ? [] : ["BUILD-INFO-windows.json"]),
    ...desktop.filter((name) => !name.endsWith(".yml")).map((name) => `${name}.blockmap`),
  ]);
  for (const infoFile of ["BUILD-INFO.json", "BUILD-INFO-windows.json"].filter((name) =>
    names.includes(name),
  )) {
    const info = JSON.parse(await readFile(path.join(directory, infoFile), "utf8"));
    assert.equal(
      info.version,
      buildLabel ?? version,
      "Build information version differs from release",
    );
    assert.equal(
      info.releaseTag,
      buildLabel ? "dev" : `v${version}`,
      "Build information tag differs from release",
    );
    if (buildLabel)
      assert.equal(
        info.packagedVersion,
        devPackagedVersion(buildLabel),
        "Packaged version differs from release",
      );
    assert.match(info.sourceCommit, /^[0-9a-f]{40}$/, "Build source must be a full commit SHA");
    assert.match(
      info.workflowCommit,
      /^[0-9a-f]{40}$/,
      "Workflow source must be a full commit SHA",
    );
  }
  for (const name of required) assert(names.includes(name), `Missing release asset: ${name}`);
  for (const name of names) {
    assert(allowed.has(name), `Unexpected release asset: ${name}`);
    const info = await stat(path.join(directory, name));
    assert(info.isFile() && info.size > 0, `Empty or invalid release asset: ${name}`);
    if (daemonAssetNames(version, buildLabel).includes(name)) {
      verifyDaemonArchive(
        path.join(directory, name),
        sourceLayout ? resolveReleaseLayout(sourceLayout).ompBridge : true,
      );
    }
  }
  const sums = [];
  for (const name of names)
    sums.push(`${await digest(path.join(directory, name), "sha256", "hex")}  ${name}`);
  await writeFile(path.join(directory, "SHA256SUMS"), `${sums.join("\n")}\n`);
  return names;
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  let buildLabel;
  if (args.at(-2) === "--dev") {
    buildLabel = args.pop();
    args.pop();
  }
  let sourceLayout;
  const sourceIndex = args.indexOf("--source-layout");
  if (sourceIndex !== -1) {
    assert(args[sourceIndex + 1], "--source-layout requires a source directory");
    sourceLayout = args[sourceIndex + 1];
    args.splice(sourceIndex, 2);
  }
  const options = args.slice(2);
  if (command === "collect" && args.length === 4) {
    const [platform, version, source, destination] = args;
    console.log(
      await collectReleaseAssets({
        platform,
        version,
        source,
        destination,
        buildLabel,
      }),
    );
  } else if (command === "daemon" && args.length === 4) {
    const [platform, version, binary, destination] = args;
    console.log(
      await packDaemonAsset({
        platform,
        version,
        binary,
        destination,
        license: "LICENSE",
        buildLabel,
        sourceLayout,
      }),
    );
  } else if (
    command === "verify" &&
    args.length >= 2 &&
    options.every((option) => ["--android", "--daemon", "--windows"].includes(option)) &&
    new Set(options).size === options.length
  ) {
    const [version, directory] = args;
    console.log(
      await verifyReleaseAssets({
        version,
        directory,
        includeAndroid: options.includes("--android"),
        includeDaemon: options.includes("--daemon"),
        includeWindows: options.includes("--windows"),
        buildLabel,
        sourceLayout,
      }),
    );
  } else {
    throw new Error(
      "Usage: release-assets.mjs collect linux|mac|win VERSION SOURCE DEST [--dev LABEL] | daemon linux|mac VERSION BINARY DEST [--source-layout ROOT] [--dev LABEL] | verify VERSION DIRECTORY [--android] [--daemon] [--windows] [--source-layout ROOT] [--dev LABEL]",
    );
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  await main();
