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
} = require("../apps/desktop/scripts/verify-packaged-resources.cjs");

function resources(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ait-package-resources-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "bin"));
  fs.writeFileSync(path.join(root, "bin", "daemon"), "daemon");
  fs.chmodSync(path.join(root, "bin", "daemon"), 0o755);
  return root;
}

test("the packaged bin directory allows exactly one executable daemon", (t) => {
  assert.doesNotThrow(() => verifyDaemonDirectory(resources(t)));
});

test("packaging fails if an old binary or CLI shim enters resources/bin", (t) => {
  for (const name of ["ait-daemon", "ait-worker", "ait", "paseo"]) {
    const root = resources(t);
    fs.writeFileSync(path.join(root, "bin", name), "unwanted");
    assert.throws(() => verifyDaemonDirectory(root), /only daemon/);
  }
});

test("packaging rejects a missing or non-executable daemon", (t) => {
  const root = resources(t);
  fs.chmodSync(path.join(root, "bin", "daemon"), 0o644);
  assert.throws(() => verifyDaemonDirectory(root));
  fs.unlinkSync(path.join(root, "bin", "daemon"));
  assert.throws(() => verifyDaemonDirectory(root), /only daemon/);
});

test("packaging requires Ait's independent identity instead of the Paseo workspace name", (t) => {
  const root = resources(t);
  const appOutDir = path.join(root, "packaged");
  const destination = path.join(appOutDir, "resources");
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
  fs.cpSync(path.join(root, "bin"), path.join(destination, "bin"), { recursive: true });
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
