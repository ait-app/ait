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
import { releaseChannel } from "./release-version.mjs";

export function releaseAssetNames(platform, version, buildLabel) {
  const channel = releaseChannel(version);
  if (buildLabel !== undefined)
    assert.match(buildLabel, /^[0-9a-f]{8}-\d{4}-\d{2}-\d{2}$/, "Invalid nightly build label");
  const label = buildLabel ?? version;
  if (platform === "linux")
    return [
      buildLabel ? `Ait-${label}-linux-x86_64.AppImage` : `Ait-linux-x86_64.AppImage`,
      `Ait-${label}-linux-x64.tar.gz`,
      `${channel}-linux.yml`,
    ];
  if (platform === "mac")
    return [`Ait-${label}-macos-arm64.dmg`, `Ait-${label}-macos-arm64.zip`, `${channel}-mac.yml`];
  if (platform === "android" && !buildLabel) return [`Ait-${version}-android.apk`];
  throw new Error(`Unsupported release platform: ${platform}`);
}

// Standalone headless daemon archives; the target suffix keeps them apart from desktop tarballs.
const daemonTargets = { linux: "linux-x64", mac: "macos-arm64" };
export const daemonExecutable = "ait-daemon";

export function daemonAssetName(platform, version) {
  releaseChannel(version);
  assert(daemonTargets[platform], `Unsupported daemon platform: ${platform}`);
  return `Ait-${version}-daemon-${daemonTargets[platform]}.tar.gz`;
}

export const daemonAssetNames = (version) =>
  Object.keys(daemonTargets).map((platform) => daemonAssetName(platform, version));

// Stable releases also carry fixed-name copies so releases/latest/download/<name> and the
// COS latest objects never need a version in the link. Maps each copy to its versioned source.
export function latestAssetNames(version) {
  assert.equal(releaseChannel(version), "latest", "Only stable releases publish latest copies");
  return {
    "Ait-macos-arm64.dmg": `Ait-${version}-macos-arm64.dmg`,
    ...Object.fromEntries(
      Object.entries(daemonTargets).map(([platform, target]) => [
        `Ait-daemon-${target}.tar.gz`,
        daemonAssetName(platform, version),
      ]),
    ),
  };
}

export async function copyLatestAssets({ version, directory }) {
  const copies = latestAssetNames(version);
  for (const [name, source] of Object.entries(copies))
    await copyFile(path.join(directory, source), path.join(directory, name));
  return Object.keys(copies);
}

export async function packDaemonAsset({ platform, version, binary, destination, license }) {
  const name = daemonAssetName(platform, version);
  // Old immutable tags name the binary `server`; only the reported version must match.
  const reported = execFileSync(binary, ["--version"], {
    encoding: "utf8",
  }).trim();
  assert.equal(reported.split(/\s+/).at(-1), version, `Daemon binary reports ${reported}`);
  const staging = await mkdtemp(path.join(tmpdir(), "ait-daemon-"));
  try {
    await copyFile(binary, path.join(staging, daemonExecutable));
    await chmod(path.join(staging, daemonExecutable), 0o755);
    const files = [daemonExecutable];
    if (license) {
      await copyFile(license, path.join(staging, "LICENSE"));
      files.push("LICENSE");
    }
    await mkdir(destination, { recursive: true });
    execFileSync("tar", ["-czf", path.resolve(destination, name), "-C", staging, ...files]);
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
  return name;
}

async function digest(file, algorithm, encoding) {
  const hash = createHash(algorithm);
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest(encoding);
}

export async function collectReleaseAssets({ platform, version, source, destination, buildLabel }) {
  const names = releaseAssetNames(platform, version, buildLabel);
  for (const name of names)
    assert((await stat(path.join(source, name))).size > 0, `Empty release asset: ${name}`);
  const { parse } = await import("yaml");
  const metadata = parse(await readFile(path.join(source, names[2]), "utf8"));
  assert.equal(metadata.version, version, "Updater metadata version differs from release");
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
    assert(names.slice(0, 2).includes(file.url), `Unexpected updater asset: ${file.url}`);
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
    assert(names.slice(0, 2).includes(metadata.path), `Unexpected updater path: ${metadata.path}`);
    assert.equal(
      await digest(path.join(source, metadata.path), "sha512", "base64"),
      metadata.sha512,
      "Legacy updater checksum mismatch",
    );
  }
  const existing = new Set(await readdir(source));
  const blockmaps = names
    .slice(0, 2)
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
  includeLatest = false,
  buildLabel,
}) {
  assert(!(includeAndroid && buildLabel), "Nightly builds support desktop platforms only");
  assert(!(includeDaemon && buildLabel), "Nightly builds do not publish standalone daemons");
  assert(!(includeLatest && buildLabel), "Nightly builds do not publish latest copies");
  const latest =
    buildLabel || releaseChannel(version) !== "latest" ? {} : latestAssetNames(version);
  const desktop = [
    ...releaseAssetNames("linux", version, buildLabel),
    ...releaseAssetNames("mac", version, buildLabel),
  ];
  const required = [
    ...desktop,
    ...(includeAndroid ? releaseAssetNames("android", version) : []),
    ...(includeDaemon ? daemonAssetNames(version) : []),
    ...(includeLatest ? Object.keys(latest) : []),
  ];
  // Android repairs re-verify stable releases that may or may not carry daemon archives.
  const allowed = new Set([
    ...required,
    ...(buildLabel ? [] : daemonAssetNames(version)),
    ...Object.keys(latest),
    "BUILD-INFO.json",
    ...desktop.filter((name) => !name.endsWith(".yml")).map((name) => `${name}.blockmap`),
  ]);
  const names = (await readdir(directory)).filter((name) => name !== "SHA256SUMS").sort();
  if (names.includes("BUILD-INFO.json")) {
    const info = JSON.parse(await readFile(path.join(directory, "BUILD-INFO.json"), "utf8"));
    assert.equal(
      info.version,
      buildLabel ?? version,
      "Build information version differs from release",
    );
    assert.equal(
      info.releaseTag,
      buildLabel ? "nightly" : `v${version}`,
      "Build information tag differs from release",
    );
    if (buildLabel)
      assert.equal(info.packagedVersion, version, "Packaged version differs from release");
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
  }
  const sums = new Map();
  for (const name of names)
    sums.set(name, await digest(path.join(directory, name), "sha256", "hex"));
  for (const [name, source] of Object.entries(latest)) {
    if (!sums.has(name)) continue;
    assert(sums.has(source), `Missing release asset: ${source}`);
    assert.equal(sums.get(name), sums.get(source), `Latest copy differs from ${source}: ${name}`);
  }
  const lines = [...sums].map(([name, hash]) => `${hash}  ${name}`);
  await writeFile(path.join(directory, "SHA256SUMS"), `${lines.join("\n")}\n`);
  return names;
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  let buildLabel;
  if (args.at(-2) === "--nightly") {
    buildLabel = args.pop();
    args.pop();
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
  } else if (command === "daemon" && args.length === 4 && !buildLabel) {
    const [platform, version, binary, destination] = args;
    console.log(
      await packDaemonAsset({
        platform,
        version,
        binary,
        destination,
        license: "LICENSE",
      }),
    );
  } else if (command === "latest" && args.length === 2 && !buildLabel) {
    const [version, directory] = args;
    console.log(await copyLatestAssets({ version, directory }));
  } else if (
    command === "verify" &&
    args.length >= 2 &&
    options.every((option) => ["--android", "--daemon", "--latest"].includes(option)) &&
    new Set(options).size === options.length
  ) {
    const [version, directory] = args;
    console.log(
      await verifyReleaseAssets({
        version,
        directory,
        includeAndroid: options.includes("--android"),
        includeDaemon: options.includes("--daemon"),
        includeLatest: options.includes("--latest"),
        buildLabel,
      }),
    );
  } else {
    throw new Error(
      "Usage: release-assets.mjs collect linux|mac VERSION SOURCE DEST [--nightly LABEL] | daemon linux|mac VERSION BINARY DEST | latest VERSION DIRECTORY | verify VERSION DIRECTORY [--android] [--daemon] [--latest] | verify VERSION DIRECTORY --nightly LABEL",
    );
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  await main();
