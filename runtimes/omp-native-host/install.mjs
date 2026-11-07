import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { constants } from "node:fs";
import {
  access,
  chmod,
  copyFile,
  link,
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

// A lock older than this was abandoned by a process that will never clean up: the longest
// legitimate install is bounded by the daemon's own installer budget.
const LOCK_HOLD_LIMIT_MS = 30 * 60 * 1000;
// An unreadable lock is a truncated write or a file left by an older installer that never
// recorded its holder. Both are only reclaimed after no real install could still be running.
const LOCK_UNREADABLE_LIMIT_MS = 10 * 60 * 1000;
// Poll interval while another installer holds the lock.
const LOCK_POLL_MS = 500;

/**
 * Whether the process recorded in a lock file is still the one that wrote it.
 *
 * A pid alone is not enough: pids are recycled, so a stale lock would look alive whenever the
 * number now belongs to something else. On Linux `/proc/<pid>` reports both the kernel start time
 * and the command name, so both must still match the record. Elsewhere only a signal probe is
 * available; the age limit covers the pid-reuse case there.
 *
 * @param {number} pid - Process id recorded in the lock.
 * @param {string} boot - Kernel start time recorded for that pid (Linux only).
 * @param {string} command - Command name recorded for that pid (Linux only).
 * @returns {Promise<boolean>} `true` only when the recorded holder still runs.
 */
async function holderAlive(pid, boot, command) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return false;
  if (process.platform !== "linux") {
    try {
      process.kill(pid, 0);
      return true;
    } catch (error) {
      // EPERM means the process exists but belongs to another user.
      return error.code === "EPERM";
    }
  }
  let stat;
  try {
    stat = await readFile(`/proc/${pid}/stat`, "utf8");
  } catch {
    return false;
  }
  // The command name is parenthesized and may itself contain spaces or parentheses.
  const open = stat.indexOf("(");
  const close = stat.lastIndexOf(")");
  const fields = stat.slice(close + 2).split(" ");
  // starttime is the 22nd field, and `fields[0]` is the 3rd.
  const alive =
    fields[19] === boot &&
    stat.slice(open + 1, close) === command &&
    // A zombie is dead for our purposes even though /proc still lists it.
    fields[0] !== "Z" &&
    fields[0] !== "X";
  return alive;
}

/** Delete `lockPath` only if it still holds exactly `expected`. */
async function removeUnchanged(lockPath, expected) {
  try {
    if ((await readFile(lockPath, "utf8")) !== expected) return;
  } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  await rm(lockPath, { force: true });
}

/**
 * Remove a lock whose recorded holder is gone or whose timestamp is older than any real run.
 *
 * The removal re-checks the exact bytes first, so a lock another installer acquired in the
 * meantime is never deleted.
 *
 * @param {string} lockPath - Lock file to inspect.
 * @returns {Promise<boolean>} `true` when the lock is gone and may be recreated.
 */
export async function reclaimLock(lockPath) {
  let content;
  let age;
  try {
    content = await readFile(lockPath, "utf8");
    age = Date.now() - (await stat(lockPath)).mtimeMs;
  } catch (error) {
    if (error.code === "ENOENT") return true;
    throw error;
  }
  let record;
  try {
    record = JSON.parse(content);
  } catch {
    record = null;
  }
  if (record === null || typeof record !== "object") {
    if (age < LOCK_UNREADABLE_LIMIT_MS) return false;
  } else {
    const startedAt = typeof record.startedAt === "number" ? record.startedAt : Number.NaN;
    const aged = !Number.isFinite(startedAt) || Date.now() - startedAt > LOCK_HOLD_LIMIT_MS;
    if (!aged && (await holderAlive(record.pid, record.boot, record.command))) return false;
  }
  await removeUnchanged(lockPath, content);
  return true;
}

/**
 * Take the installation lock, waiting for a live holder to finish.
 *
 * Concurrent daemons must not install into the same tree: the loser waits for the winner and
 * then reuses its result instead of writing a second tree. A lock whose holder died, or one older
 * than any real install, is reclaimed instead of blocking forever. Waiting is bounded well below
 * the daemon's installer budget, so one daemon can never pin another past its own deadline; the
 * daemon then reports the degradation and tries again on its next start.
 *
 * @param {string} lockPath - Lock file beside the destination.
 * @param {number} [waitMs] - How long to wait for a live holder before failing.
 * @returns {Promise<{handle: import("node:fs/promises").FileHandle, record: string}>} The
 *   exclusive lock handle and the exact bytes it holds.
 */
export async function acquireLock(lockPath, waitMs = 60 * 1000) {
  const record = `${JSON.stringify(await lockHolder())}\n`;
  const candidate = `${lockPath}.${process.pid}.holder`;
  await writeFile(candidate, record, { flag: "wx", mode: 0o600 });
  const deadline = Date.now() + waitMs;
  try {
    for (;;) {
      const handle = await publishLock(candidate, record, lockPath);
      if (handle) return { handle, record };
      if (await reclaimLock(lockPath)) continue;
      if (Date.now() >= deadline) {
        throw new Error(
          `Another OMP native host install still holds ${lockPath}; this run leaves it alone`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, LOCK_POLL_MS));
    }
  } finally {
    await rm(candidate, { force: true });
  }
}

/**
 * Publish the holder record as the lock, or report that someone else owns it.
 *
 * A hard link is atomic and readers never observe a partly written holder. Filesystems without
 * links fall back to an exclusive create: a reader may briefly see an empty lock, which it treats
 * as a live install until the age limit rather than deleting.
 *
 * @param {string} candidate - File already holding `record`.
 * @param {string} record - Exact lock contents.
 * @param {string} lockPath - Lock to create.
 * @returns {Promise<import("node:fs/promises").FileHandle|null>} Created lock handle, or `null`.
 */
async function publishLock(candidate, record, lockPath) {
  try {
    await link(candidate, lockPath);
    return await open(lockPath, "r");
  } catch (error) {
    if (error.code === "EEXIST") return null;
    if (!["EPERM", "ENOSYS", "EXDEV", "EMLINK", "ENOTSUP", "EOPNOTSUPP"].includes(error.code)) {
      throw error;
    }
  }
  try {
    const handle = await open(lockPath, "wx", 0o600);
    await handle.writeFile(record);
    return handle;
  } catch (error) {
    if (error.code === "EEXIST") return null;
    throw error;
  }
}

/**
 * Describe this process for the lock file so a later run can tell whether it is still alive.
 *
 * @returns {Promise<{pid: number, startedAt: number, boot: string, command: string}>} Holder id.
 */
export async function lockHolder() {
  const holder = { pid: process.pid, startedAt: Date.now(), boot: "", command: "" };
  if (process.platform === "linux") {
    const stat = await readFile(`/proc/${process.pid}/stat`, "utf8").catch(() => "");
    const close = stat.lastIndexOf(")");
    holder.boot = stat.slice(close + 2).split(" ")[19] ?? "";
    holder.command = stat.slice(stat.indexOf("(") + 1, close);
  }
  return holder;
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
  const guard = await acquireLock(lockPath);
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
    await guard.handle.close();
    // Only remove the lock this run still owns; a reclaimed lock may already belong elsewhere.
    await removeUnchanged(lockPath, guard.record);
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
