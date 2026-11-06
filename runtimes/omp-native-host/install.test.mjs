import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { bunPackage, parseInstallArgs, publishInstallation, validateLock } from "./install.mjs";

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
  assert.deepEqual(parseInstallArgs(["--destination", "/tmp/host", "--platform", "win32", "--arch", "x64"]), {
    destination: path.resolve("/tmp/host"), platform: "win32", arch: "x64",
  });
  for (const args of [[], ["--data-dir"], ["--data-dir", "/tmp/a", "--destination", "/tmp/b"], ["--platform", "linux"], ["--destination", "/tmp/a", "--unknown", "value"]]) {
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
  await assert.rejects(publishInstallation(path.join(root, "missing-stage"), destination), /ENOENT/);
  assert.equal(await readFile(path.join(destination, "manifest.json"), "utf8"), "previous manifest");
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
  await assert.rejects(patchOfficialRuntime(path.join(root, "missing")), /package\.json is unreadable/);
  const sdk = path.join(root, "sdk");
  await mkdir(sdk);
  await writeFile(path.join(sdk, "package.json"), JSON.stringify({ name: "@oh-my-pi/pi-coding-agent", version: "18.17.0" }));
  await assert.rejects(patchOfficialRuntime(sdk), /import target is missing/);
  for (const file of [
    "src/config/model-resolver.ts", "src/session/agent-session.ts", "src/slash-commands/available-commands.ts",
    "src/vibe/runtime.ts", "src/vibe/lifecycle.ts", "src/registry/agent-registry.ts",
    "src/session/session-loader.ts", "src/modes/rpc/rpc-subagents.ts", "src/modes/rpc/rpc-btw.ts",
    "src/modes/rpc/rpc-mode.ts", "src/main.ts",
  ]) {
    await mkdir(path.dirname(path.join(sdk, file)), { recursive: true });
    await writeFile(path.join(sdk, file), "export {};\n");
  }
  await assert.rejects(patchOfficialRuntime(sdk), /not the expected 18\.17\.0 source/);
  await writeFile(path.join(sdk, "package.json"), JSON.stringify({ name: "@oh-my-pi/pi-coding-agent", version: "18.18.0" }));
  await assert.rejects(patchOfficialRuntime(sdk), /Expected 18\.17\.0/);
});
