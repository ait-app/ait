import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { constants } from "node:fs";
import {
  access,
  chmod,
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  open,
  readFile,
  readdir,
  realpath,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const assets = fileURLToPath(new URL(".", import.meta.url));
export const OMP_VERSION = "18.17.0";
export const BUN_VERSION = "1.4.4";
export const SDK_DIRECTORY = "node_modules/@oh-my-pi/pi-coding-agent";
export const ENTRYPOINT = `${SDK_DIRECTORY}/src/cli.ts`;
const SDK_INTEGRITY =
  "sha512-wadF0I1p6YsY7AwNrM+espeaaodCUWpl4Hs23aVcYArB/lUdG4NUQ74Da7oc+xUEQq1LkbuJCtIpUaF4NgUudg==";

export function bunPackage(platform = process.platform, arch = process.arch, libc) {
  assert(
    ["darwin", "linux", "win32"].includes(platform),
    `Unsupported runtime platform: ${platform}`,
  );
  assert(["x64", "arm64"].includes(arch), `Unsupported runtime architecture: ${arch}`);
  const os = platform === "win32" ? "windows" : platform;
  const cpu = arch === "arm64" ? "aarch64" : arch;
  return `@oven/bun-${os}-${cpu}${platform === "linux" && libc === "musl" ? "-musl" : ""}`;
}

export function validateLock(lock, pkg) {
  assert.equal(lock.lockfileVersion, 3, "Runtime requires npm lockfile v3");
  assert.deepEqual(pkg.dependencies, {
    "@oh-my-pi/pi-coding-agent": OMP_VERSION,
    bun: BUN_VERSION,
  });
  assert.deepEqual(lock.packages[""].dependencies, pkg.dependencies);
  for (const [location, entry] of Object.entries(lock.packages)) {
    if (!location) continue;
    assert(
      location.startsWith("node_modules/") && !location.split("/").includes(".."),
      `Invalid locked path: ${location}`,
    );
    assert.equal(
      new URL(entry.resolved).origin,
      "https://registry.npmjs.org",
      `Non-published dependency: ${location}`,
    );
    assert.match(
      entry.integrity,
      /^sha512-[A-Za-z0-9+/]+={0,2}$/,
      `Missing npm integrity: ${location}`,
    );
    assert(!entry.link, `Linked dependency is forbidden: ${location}`);
  }
  const sdk = lock.packages[SDK_DIRECTORY];
  assert.equal(sdk.version, OMP_VERSION);
  assert.equal(sdk.integrity, SDK_INTEGRITY);
  assert.equal(lock.packages["node_modules/bun"].version, BUN_VERSION);
  for (const platform of ["darwin", "linux", "win32"]) {
    for (const arch of ["x64", "arm64"]) {
      assert.equal(
        lock.packages[`node_modules/${bunPackage(platform, arch)}`].version,
        BUN_VERSION,
      );
      assert.equal(
        lock.packages[`node_modules/@oh-my-pi/pi-natives-${platform}-${arch}`].version,
        OMP_VERSION,
      );
    }
  }
}

async function exists(file) {
  try {
    await lstat(file);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

// Only the final rename changes the active installation. Restore it if publication fails.
export async function publishInstallation(staging, destination) {
  const backup = `${staging}.previous`;
  const hadPrevious = await exists(destination);
  if (hadPrevious) await rename(destination, backup);
  try {
    await rename(staging, destination);
  } catch (error) {
    if (hadPrevious) await rename(backup, destination);
    throw error;
  }
  if (hadPrevious) {
    // Publication has committed. A cleanup problem must not misreport install failure.
    await rm(backup, { recursive: true, force: true }).catch((error) => {
      console.warn(
        `Installed native host; could not remove previous installation ${backup}: ${error.message}`,
      );
    });
  }
}

async function npmCommand() {
  if (process.env.npm_execpath && /\.[cm]?js$/.test(process.env.npm_execpath)) {
    return [process.execPath, [process.env.npm_execpath]];
  }
  if (process.platform !== "win32") return ["npm", []];
  // Execute npm's JS entrypoint, never a shell-expanded .cmd with user paths.
  const candidates = [
    path.dirname(process.execPath),
    ...(process.env.PATH ?? "").split(path.delimiter),
  ];
  for (const directory of candidates) {
    const cli = path.join(directory, "node_modules/npm/bin/npm-cli.js");
    if (await exists(cli)) return [process.execPath, [cli]];
  }
  throw new Error(
    "Install Node.js with npm, or invoke this installer through npm run install:omp-native-host.",
  );
}

async function stageNotices(staging, lock) {
  const notices = path.join(staging, "notices");
  await mkdir(notices);
  const upstreamLicense = path.join(staging, SDK_DIRECTORY, "LICENSE");
  await copyFile(upstreamLicense, path.join(notices, "oh-my-pi-LICENSE"));
  for (const entry of await readdir(path.join(assets, "licenses"), { withFileTypes: true })) {
    assert(entry.isFile(), "Runtime license assets must be regular files");
    await copyFile(path.join(assets, "licenses", entry.name), path.join(notices, entry.name));
  }
  const dependencies = [];
  for (const [location, entry] of Object.entries(lock.packages)) {
    if (!location || !(await exists(path.join(staging, location, "package.json")))) continue;
    dependencies.push({
      path: location,
      version: entry.version,
      license: entry.license ?? "See package notices",
      source: entry.resolved,
      integrity: entry.integrity,
    });
  }
  await writeFile(
    path.join(notices, "dependencies.json"),
    `${JSON.stringify(dependencies, null, 2)}\n`,
  );
  await writeFile(
    path.join(notices, "NOTICE.txt"),
    `Ait private native OMP host adapter, version 1.\n` +
      `Oh My Pi ${OMP_VERSION}, MIT, https://github.com/can1357/oh-my-pi\n` +
      `Published SDK source is modified only by the accompanying Ait RPC UI adapter patch.\n` +
      `The upstream MIT copyright and permission notice is in oh-my-pi-LICENSE.\n` +
      `Bun ${BUN_VERSION}, MIT, https://github.com/oven-sh/bun/tree/bun-v${BUN_VERSION}\n` +
      `Bun includes JavaScriptCore/WebKit (LGPL) and other linked dependencies.\n` +
      `See bun-upstream.txt and the accompanying dependency license assets for attribution and relinking instructions.\n` +
      `All npm dependency license, NOTICE, copyright and source assets remain inside node_modules; dependencies.json records their locked provenance.\n`,
  );
}

export async function installRuntime({
  destination,
  platform = process.platform,
  arch = process.arch,
}) {
  const native = platform === process.platform && arch === process.arch;
  const libc =
    platform === "linux" && native && !process.report.getReport().header.glibcVersionRuntime
      ? "musl"
      : "glibc";
  const binaryPackage = bunPackage(platform, arch, libc);
  destination = path.resolve(destination);
  const parent = path.dirname(destination);
  await mkdir(parent, { recursive: true });
  const lockPath = path.join(parent, `.${path.basename(destination)}.install-lock`);
  const guard = await open(lockPath, "wx", 0o600);
  let staging;
  try {
    const pkg = JSON.parse(await readFile(path.join(assets, "package.json"), "utf8"));
    const lock = JSON.parse(await readFile(path.join(assets, "package-lock.json"), "utf8"));
    validateLock(lock, pkg);
    staging = await mkdtemp(path.join(parent, `.${path.basename(destination)}.install-`));
    for (const filename of ["package.json", "package-lock.json"]) {
      await copyFile(path.join(assets, filename), path.join(staging, filename));
    }
    const [npm, prefix] = await npmCommand();
    execFileSync(
      npm,
      [
        ...prefix,
        "ci",
        "--prefix",
        staging,
        "--workspaces=false",
        "--ignore-scripts",
        "--include=optional",
        "--omit=dev",
        "--no-audit",
        "--no-fund",
        "--registry=https://registry.npmjs.org",
        `--os=${platform}`,
        `--cpu=${arch}`,
        `--libc=${libc}`,
      ],
      {
        cwd: staging,
        stdio: "inherit",
      },
    );
    const bunPath = `node_modules/${binaryPackage}/bin/${platform === "win32" ? "bun.exe" : "bun"}`;
    const bun = path.join(staging, bunPath);
    const installedBun = JSON.parse(
      await readFile(path.join(staging, "node_modules", binaryPackage, "package.json"), "utf8"),
    );
    const installedSdk = JSON.parse(
      await readFile(path.join(staging, SDK_DIRECTORY, "package.json"), "utf8"),
    );
    assert.equal(installedBun.version, BUN_VERSION);
    assert.equal(installedSdk.version, OMP_VERSION);
    assert((await stat(bun)).isFile(), "Published Bun executable is missing");
    assert(
      (await realpath(bun)).startsWith(`${await realpath(staging)}${path.sep}`),
      "Bun must remain inside the isolated runtime",
    );
    const binary = await open(bun, "r");
    try {
      const header = Buffer.alloc(4);
      assert.equal((await binary.read(header, 0, 4, 0)).bytesRead, 4, "Truncated Bun executable");
      const signature = header.toString("hex");
      assert(
        platform === "win32"
          ? signature.startsWith("4d5a")
          : platform === "linux"
            ? signature === "7f454c46"
            : ["cffaedfe", "feedfacf", "cafebabe", "bebafeca"].includes(signature),
        `Bun executable does not match ${platform}`,
      );
    } finally {
      await binary.close();
    }
    if (platform !== "win32") {
      await chmod(bun, 0o755);
      await access(bun, constants.X_OK);
    }
    if (native)
      assert.equal(
        execFileSync(bun, ["--version"], { encoding: "utf8", cwd: staging }).trim(),
        BUN_VERSION,
      );
    await access(
      path.join(staging, `node_modules/@oh-my-pi/pi-natives-${platform}-${arch}/package.json`),
      constants.R_OK,
    );
    const { patchOfficialRuntime } = await import("./rpc-patch.mjs");
    await patchOfficialRuntime(path.join(staging, SDK_DIRECTORY));
    await access(path.join(staging, ENTRYPOINT), constants.R_OK);
    await stageNotices(staging, lock);
    const manifest = {
      schemaVersion: 1,
      adapter: "ait-omp-native-host",
      adapterVersion: 1,
      ompVersion: OMP_VERSION,
      bunVersion: BUN_VERSION,
      bunPath,
      entrypoint: ENTRYPOINT,
    };
    const serialized = `${JSON.stringify(manifest, null, 2)}\n`;
    assert(Buffer.byteLength(serialized) <= 4096, "Native runtime manifest exceeds its size bound");
    await writeFile(path.join(staging, "manifest.json"), serialized, { flag: "wx" });
    await publishInstallation(staging, destination);
    staging = undefined;
    return manifest;
  } finally {
    if (staging) await rm(staging, { recursive: true, force: true });
    await guard.close();
    await rm(lockPath);
  }
}

export function parseInstallArgs(args) {
  const options = {};
  assert(
    args.length >= 2 && args.length % 2 === 0,
    "Usage: node install.mjs --data-dir AIT_DATA_DIR | --destination VERSION_ROOT [--platform linux|darwin|win32] [--arch x64|arm64]",
  );
  for (let i = 0; i < args.length; i += 2) {
    const [option, value] = args.slice(i, i + 2);
    assert(value && !value.startsWith("--"), `Missing value for ${option}`);
    if (option === "--data-dir" || option === "--destination") {
      assert(!options.destination, "Choose exactly one installation destination");
      options.destination =
        option === "--data-dir"
          ? path.resolve(value, "providers/omp-native-host/1")
          : path.resolve(value);
    } else {
      assert(["--platform", "--arch"].includes(option), `Unknown installer option: ${option}`);
      const key = option.slice(2);
      assert(!options[key], `Duplicate option: ${option}`);
      options[key] = value;
    }
  }
  assert(options.destination, "An installation destination is required");
  bunPackage(options.platform, options.arch);
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const options = parseInstallArgs(process.argv.slice(2));
  await installRuntime(options);
  console.log(`Installed private OMP ${OMP_VERSION} / Bun ${BUN_VERSION}: ${options.destination}`);
}
