const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { uncache } = require("@electron/asar");
const {
  verifyPackagedResources,
  verifyDaemonDirectory,
  verifyNativeHostDirectory,
} = require("../apps/desktop/scripts/verify-packaged-resources.cjs");

function nativeHost(resources, platform = "linux") {
  const root = path.join(resources, "provider-runtimes/omp-native-host/1");
  const bunPath =
    platform === "win32"
      ? "node_modules/@oven/bun-windows-x64/bin/bun.exe"
      : "node_modules/@oven/bun-linux-x64/bin/bun";
  const entrypoint = "node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts";
  const patched = [
    "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-hello.ts",
    "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-bridge.ts",
  ];
  for (const relative of [
    bunPath,
    entrypoint,
    ...patched,
    "notices/oh-my-pi-LICENSE",
    "notices/bun-upstream.txt",
    "notices/bun-linked-dependencies.txt",
    "notices/dependencies.json",
    "notices/NOTICE.txt",
  ]) {
    const filename = path.join(root, relative);
    fs.mkdirSync(path.dirname(filename), { recursive: true });
    fs.writeFileSync(filename, "fixture", { mode: 0o755 });
  }
  fs.writeFileSync(
    path.join(root, "manifest.json"),
    JSON.stringify({
      schemaVersion: 1,
      adapter: "ait-omp-native-host",
      adapterVersion: 1,
      ompVersion: "18.17.0",
      bunVersion: "1.4.4",
      bunPath,
      entrypoint,
    }),
  );
  return root;
}

function resources(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ait-package-resources-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "bin"));
  fs.writeFileSync(path.join(root, "bin", "aitd"), "daemon");
  fs.chmodSync(path.join(root, "bin", "aitd"), 0o755);
  return root;
}

test("the packaged bin directory allows exactly one executable daemon", (t) => {
  assert.doesNotThrow(() => verifyDaemonDirectory(resources(t)));
});

test("packaging fails if an old binary or CLI shim enters resources/bin", (t) => {
  // The former bundled name `daemon` must not ship next to `aitd`.
  for (const name of ["daemon", "ait-daemon", "ait-worker", "ait", "paseo"]) {
    const root = resources(t);
    fs.writeFileSync(path.join(root, "bin", name), "unwanted");
    assert.throws(() => verifyDaemonDirectory(root), /only aitd/);
  }
});

test("packaging rejects a missing or non-executable daemon", (t) => {
  const root = resources(t);
  fs.chmodSync(path.join(root, "bin", "aitd"), 0o644);
  assert.throws(() => verifyDaemonDirectory(root));
  fs.unlinkSync(path.join(root, "bin", "aitd"));
  assert.throws(() => verifyDaemonDirectory(root), /only aitd/);
});

test("packaging requires Ait's independent identity instead of the Paseo workspace name", (t) => {
  const root = resources(t);
  const appOutDir = path.join(root, "packaged");
  const destination = path.join(appOutDir, "resources");
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
  fs.cpSync(path.join(root, "bin"), path.join(destination, "bin"), { recursive: true });
  nativeHost(destination);
  const source = path.join(root, "source");
  fs.mkdirSync(source);
  for (const name of ["@ait/desktop", "@getpaseo/desktop"]) {
    fs.writeFileSync(path.join(source, "package.json"), JSON.stringify({ name, version: "0.0.7" }));
    // Wait for the CLI to flush the ASAR; the v3 library promise can resolve before file writes.
    execFileSync(process.execPath, [
      require.resolve("@electron/asar/bin/asar.js"),
      "pack",
      source,
      path.join(destination, "app.asar"),
    ]);
    uncache(path.join(destination, "app.asar"));
    const verify = () =>
      verifyPackagedResources({ appOutDir, platform: "linux", version: "0.0.7" });
    if (name === "@ait/desktop") assert.doesNotThrow(verify);
    else assert.throws(verify, /independent package identity/);
  }
});

test("Windows packages require the renderer and app archive but reject a bundled daemon", (t) => {
  const root = resources(t);
  const appOutDir = path.join(root, "windows");
  const destination = path.join(appOutDir, "resources");
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
  nativeHost(destination, "win32");
  const source = path.join(root, "source");
  fs.mkdirSync(source);
  fs.writeFileSync(
    path.join(source, "package.json"),
    JSON.stringify({ name: "@ait/desktop", version: "0.1.15" }),
  );
  execFileSync(process.execPath, [
    require.resolve("@electron/asar/bin/asar.js"),
    "pack",
    source,
    path.join(destination, "app.asar"),
  ]);
  const verify = () => verifyPackagedResources({ appOutDir, platform: "win32", version: "0.1.15" });
  assert.doesNotThrow(verify);
  fs.cpSync(path.join(root, "bin"), path.join(destination, "bin"), { recursive: true });
  assert.throws(verify, /must not bundle a daemon/);
  fs.rmSync(path.join(destination, "bin"), { recursive: true });
  fs.unlinkSync(path.join(destination, "app-dist", "index.html"));
  assert.throws(verify, /UI is missing/);
});

test("private native host packaging rejects wrong versions, missing notices and path escapes", (t) => {
  const root = nativeHost(resources(t));
  assert.doesNotThrow(() => verifyNativeHostDirectory(root, "linux"));
  assert.throws(() => verifyNativeHostDirectory(root, "win32"), /target differs/);
  const manifestPath = path.join(root, "manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  fs.writeFileSync(manifestPath, JSON.stringify({ ...manifest, ompVersion: "18.14.0" }));
  assert.throws(() => verifyNativeHostDirectory(root, "linux"));
  fs.writeFileSync(manifestPath, JSON.stringify({ ...manifest, bunPath: "../../bun" }));
  assert.throws(() => verifyNativeHostDirectory(root, "linux"));
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  fs.unlinkSync(path.join(root, "notices/bun-linked-dependencies.txt"));
  assert.throws(() => verifyNativeHostDirectory(root, "linux"), /Missing native host asset/);
});

test("packaging rejects a native host SDK that is not patched with the Ait bridge", (t) => {
  const root = nativeHost(resources(t));
  assert.doesNotThrow(() => verifyNativeHostDirectory(root, "linux"));
  fs.unlinkSync(
    path.join(
      root,
      "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-hello.ts",
    ),
  );
  assert.throws(() => verifyNativeHostDirectory(root, "linux"), /Missing native host asset/);
});
