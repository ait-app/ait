import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, readdir, rm, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  acquireLock,
  bunPackage,
  lockHolder,
  parseInstallArgs,
  publishInstallation,
  reclaimLock,
  validateLock,
} from "./install.mjs";

const pkg = JSON.parse(await readFile(new URL("./package.json", import.meta.url), "utf8"));
const lock = JSON.parse(await readFile(new URL("./package-lock.json", import.meta.url), "utf8"));

test("the standalone lock pins published SDK and every supported Bun/native platform", () => {
  assert.doesNotThrow(() => validateLock(lock, pkg));
  const changed = structuredClone(lock);
  changed.packages["node_modules/@oh-my-pi/pi-coding-agent"].integrity = "sha512-replaced";
  assert.throws(() => validateLock(changed, pkg));
  const local = structuredClone(lock);
  local.packages["node_modules/bun"].resolved = "file:/tmp/bun";
  assert.throws(() => validateLock(local, pkg), /Non-published dependency/);
  const missing = structuredClone(lock);
  delete missing.packages["node_modules/@oven/bun-windows-x64"];
  assert.throws(() => validateLock(missing, pkg));
});

test("Bun paths identify actual published platform packages, including cross Windows", () => {
  assert.equal(bunPackage("darwin", "arm64"), "@oven/bun-darwin-aarch64");
  assert.equal(bunPackage("win32", "x64"), "@oven/bun-windows-x64");
  assert.equal(bunPackage("linux", "arm64", "musl"), "@oven/bun-linux-aarch64-musl");
  assert.throws(() => bunPackage("linux", "ia32"), /Unsupported runtime architecture/);
});

test("CLI maps Ait data dir to its managed version root without guessing user state", () => {
  assert.deepEqual(parseInstallArgs(["--data-dir", "/tmp/ait-data"]), {
    destination: path.resolve("/tmp/ait-data/providers/omp-native-host/1"),
  });
  assert.deepEqual(
    parseInstallArgs(["--destination", "/tmp/host", "--platform", "win32", "--arch", "x64"]),
    {
      destination: path.resolve("/tmp/host"),
      platform: "win32",
      arch: "x64",
    },
  );
  for (const args of [
    [],
    ["--data-dir"],
    ["--data-dir", "/tmp/a", "--destination", "/tmp/b"],
    ["--platform", "linux"],
    ["--destination", "/tmp/a", "--unknown", "value"],
  ]) {
    assert.throws(() => parseInstallArgs(args));
  }
});

async function directory(t) {
  const root = await mkdtemp(path.join(tmpdir(), "ait-native-install-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

test("failed stage publication restores the complete previous installation", async (t) => {
  const root = await directory(t);
  const destination = path.join(root, "1");
  await mkdir(destination);
  await writeFile(path.join(destination, "manifest.json"), "previous manifest");
  await writeFile(path.join(destination, "host"), "previous host");
  await assert.rejects(
    publishInstallation(path.join(root, "missing-stage"), destination),
    /ENOENT/,
  );
  assert.equal(
    await readFile(path.join(destination, "manifest.json"), "utf8"),
    "previous manifest",
  );
  assert.equal(await readFile(path.join(destination, "host"), "utf8"), "previous host");
  assert.deepEqual(await readdir(root), ["1"]);
});

test("successful stage publication replaces rather than merges stale host files", async (t) => {
  const root = await directory(t);
  const destination = path.join(root, "1");
  const staging = path.join(root, ".1.install-stage");
  await mkdir(destination);
  await mkdir(staging);
  await writeFile(path.join(destination, "stale"), "old");
  await writeFile(path.join(staging, "manifest.json"), "complete new manifest");
  await publishInstallation(staging, destination);
  assert.deepEqual(await readdir(destination), ["manifest.json"]);
  assert.deepEqual(await readdir(root), ["1"]);
});

test("the RPC patch refuses unverified SDK trees instead of skipping", async (t) => {
  const { patchOfficialRuntime } = await import("./rpc-patch.mjs");
  const root = await directory(t);
  await assert.rejects(
    patchOfficialRuntime(path.join(root, "missing")),
    /package\.json is unreadable/,
  );
  const sdk = path.join(root, "sdk");
  await mkdir(sdk);
  await writeFile(
    path.join(sdk, "package.json"),
    JSON.stringify({ name: "@oh-my-pi/pi-coding-agent", version: "18.17.0" }),
  );
  await assert.rejects(patchOfficialRuntime(sdk), /import target is missing/);
  for (const file of [
    "src/config/model-resolver.ts",
    "src/session/agent-session.ts",
    "src/slash-commands/available-commands.ts",
    "src/vibe/runtime.ts",
    "src/vibe/lifecycle.ts",
    "src/registry/agent-registry.ts",
    "src/session/session-loader.ts",
    "src/modes/rpc/rpc-subagents.ts",
    "src/modes/rpc/rpc-btw.ts",
    "src/modes/rpc/rpc-mode.ts",
    "src/main.ts",
  ]) {
    await mkdir(path.dirname(path.join(sdk, file)), { recursive: true });
    await writeFile(path.join(sdk, file), "export {};\n");
  }
  await assert.rejects(patchOfficialRuntime(sdk), /not the expected 18\.17\.0 source/);
  await writeFile(
    path.join(sdk, "package.json"),
    JSON.stringify({ name: "@oh-my-pi/pi-coding-agent", version: "18.18.0" }),
  );
  await assert.rejects(patchOfficialRuntime(sdk), /Expected 18\.17\.0/);
});

/// Locks are the part that survives a killed installer, so they are exercised directly.
test("a lock whose holder died is reclaimed instead of blocking every later install", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  // A pid that has certainly exited: this test's process waited for it.
  const dead = spawnSync(process.execPath, ["-e", "process.exit(0)"]);
  await writeFile(
    lockPath,
    `${JSON.stringify({ pid: dead.pid, startedAt: Date.now(), boot: "0", command: "node" })}\n`,
  );
  assert.equal(await reclaimLock(lockPath), true);
  await assert.rejects(readFile(lockPath), /ENOENT/);
});

test("a lock recorded with a recycled pid is not treated as alive", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  // The pid is this live process, but the recorded start time belongs to another process.
  const stat = await readFile(`/proc/${process.pid}/stat`, "utf8");
  const close = stat.lastIndexOf(")");
  const boot = stat.slice(close + 2).split(" ")[19];
  await writeFile(
    lockPath,
    `${JSON.stringify({ pid: process.pid, startedAt: Date.now(), boot: `${boot}1`, command: "other" })}\n`,
  );
  assert.equal(await reclaimLock(lockPath), true);
});

test("a lock held by a live installer is never deleted", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  const held = `${JSON.stringify(await lockHolder())}\n`;
  await writeFile(lockPath, held);
  assert.equal(await reclaimLock(lockPath), false);
  assert.equal(await readFile(lockPath, "utf8"), held);
});

test("an aged lock is reclaimed even when its pid cannot be probed", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  const staleAt = Date.now() - 60 * 60 * 1000;
  await writeFile(
    lockPath,
    `${JSON.stringify({ pid: process.pid, startedAt: staleAt, boot: "", command: "" })}\n`,
  );
  assert.equal(await reclaimLock(lockPath), true);
});

test("an unreadable lock is only reclaimed once no install could still be running", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  await writeFile(lockPath, "{");
  assert.equal(await reclaimLock(lockPath), false);
  const stale = new Date(Date.now() - 60 * 60 * 1000);
  await utimes(lockPath, stale, stale);
  assert.equal(await reclaimLock(lockPath), true);
});

test("a second installer waits for a live holder and then reuses the result", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  const first = await acquireLock(lockPath);
  assert.equal(await reclaimLock(lockPath), false, "an own lock is live");
  let second = false;
  const waiting = acquireLock(lockPath, 8000).then((guard) => {
    second = true;
    return guard;
  });
  // Still waiting while the first holder owns the lock.
  await new Promise((resolve) => setTimeout(resolve, 700));
  assert.equal(second, false);
  await first.handle.close();
  await rm(lockPath, { force: true });
  const guard = await waiting;
  assert.equal(second, true);
  await guard.handle.close();
  await rm(lockPath, { force: true });
});

test("waiting for a live holder is bounded so one daemon cannot pin another", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  const first = await acquireLock(lockPath);
  await assert.rejects(acquireLock(lockPath, 100), /still holds/);
  await first.handle.close();
  await rm(lockPath, { force: true });
});

test("a release only removes the lock this run still owns", async (t) => {
  const root = await directory(t);
  const lockPath = path.join(root, ".1.install-lock");
  const first = await acquireLock(lockPath);
  // Another installer reclaimed and replaced the lock after the first run lost it.
  await rm(lockPath, { force: true });
  const replacement = `${JSON.stringify({ pid: 1, startedAt: Date.now(), boot: "", command: "" })}\n`;
  await writeFile(lockPath, replacement);
  await first.handle.close();
  assert.notEqual(await readFile(lockPath, "utf8"), first.record);
  assert.equal(await readFile(lockPath, "utf8"), replacement);
});
