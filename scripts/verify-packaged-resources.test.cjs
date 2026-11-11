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
  for (const name of ["aitd", "ait-omp-acp"]) {
    fs.writeFileSync(path.join(root, "bin", name), `#!/bin/sh\necho '${name} 0.0.7'\n`);
    fs.chmodSync(path.join(root, "bin", name), 0o755);
  }
  return root;
}

test("the packaged bin directory allows exactly the native daemon and bridge", (t) => {
  assert.doesNotThrow(() => verifyDaemonDirectory(resources(t), "0.0.7"));
});

test("packaging fails if an old binary or CLI shim enters resources/bin", (t) => {
  // The former bundled name `daemon` must not ship next to `aitd`.
  for (const name of ["daemon", "ait-daemon", "ait-worker", "ait", "paseo", "omp", "bun", "sdk"]) {
    const root = resources(t);
    fs.writeFileSync(path.join(root, "bin", name), "unwanted");
    assert.throws(() => verifyDaemonDirectory(root, "0.0.7"), /only aitd/);
  }
});

test("packaging rejects missing, non-executable, symlinked, or mismatched native binaries", (t) => {
  for (const name of ["aitd", "ait-omp-acp"]) {
    const root = resources(t);
    const binary = path.join(root, "bin", name);
    fs.writeFileSync(binary, `#!/bin/sh\necho '${name} 0.0.6'\n`);
    assert.throws(() => verifyDaemonDirectory(root, "0.0.7"), /version differs from desktop/);
    fs.chmodSync(binary, 0o644);
    assert.throws(() => verifyDaemonDirectory(root, "0.0.7"));
    fs.unlinkSync(binary);
    assert.throws(() => verifyDaemonDirectory(root, "0.0.7"), /only aitd/);
    fs.symlinkSync(process.execPath, binary);
    assert.throws(() => verifyDaemonDirectory(root, "0.0.7"), /regular file/);
  }
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

test("Windows packages require the renderer and app archive but reject a bundled daemon", (t) => {
  const root = resources(t);
  const appOutDir = path.join(root, "windows");
  const destination = path.join(appOutDir, "resources");
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
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

test("macOS signature verification covers both resources and the enclosing app", () => {
  const { verifyMacSignatures } = require("../apps/desktop/scripts/after-sign.js");
  const calls = [];
  verifyMacSignatures("/release", (command, args) => calls.push([command, args]));
  assert.deepEqual(
    calls.map(([command, args]) => [command, args.at(-1)]),
    [
      ["codesign", "/release/Ait.app/Contents/Resources/bin/aitd"],
      ["codesign", "/release/Ait.app/Contents/Resources/bin/ait-omp-acp"],
      ["codesign", "/release/Ait.app"],
    ],
  );
  assert(calls.every(([, args]) => args.includes("--verify") && args.includes("--strict")));
  assert(calls.at(-1)[1].includes("--deep"));
  assert.throws(
    () =>
      verifyMacSignatures("/release", (_command, args) => {
        if (args.at(-1).endsWith("ait-omp-acp")) throw new Error("invalid bridge signature");
      }),
    /invalid bridge signature/,
  );
});

test("packaged source identities match both native tools and the desktop SemVer", (t) => {
  const { packagedVersion } = require("./source-version.cjs");
  const root = resources(t);
  const appOutDir = path.join(root, "packaged");
  const destination = path.join(appOutDir, "resources");
  const source = path.join(root, "source");
  fs.mkdirSync(source);
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
  fs.cpSync(path.join(root, "bin"), path.join(destination, "bin"), { recursive: true });
  for (const displayVersion of ["git-2026-11-11-1234abcd", "2026-11-11-1234abcd"]) {
    const version = packagedVersion(displayVersion);
    for (const name of ["aitd", "ait-omp-acp"])
      fs.writeFileSync(
        path.join(destination, "bin", name),
        `#!/bin/sh\necho '${name} ${displayVersion}'\n`,
      );
    const pack = (identity) => {
      fs.writeFileSync(
        path.join(source, "package.json"),
        JSON.stringify({ name: "@ait/desktop", version, displayVersion: identity }),
      );
      execFileSync(process.execPath, [
        require.resolve("@electron/asar/bin/asar.js"),
        "pack",
        source,
        path.join(destination, "app.asar"),
      ]);
      uncache(path.join(destination, "app.asar"));
    };
    const verify = () => verifyPackagedResources({ appOutDir, platform: "linux", version });
    pack(displayVersion);
    assert.doesNotThrow(verify);
    fs.writeFileSync(
      path.join(destination, "bin/ait-omp-acp"),
      "#!/bin/sh\necho 'ait-omp-acp 0.0.7'\n",
    );
    assert.throws(verify, /ait-omp-acp version differs/);
    pack("0.0.7");
    assert.throws(verify, /display identity differs/);
  }
});

test("packaging rejects OMP SDK and Bun dependencies inside the application archive", (t) => {
  const root = resources(t);
  const appOutDir = path.join(root, "packaged");
  const destination = path.join(appOutDir, "resources");
  const source = path.join(root, "source");
  fs.mkdirSync(source);
  fs.mkdirSync(path.join(destination, "app-dist"), { recursive: true });
  fs.writeFileSync(path.join(destination, "app-dist", "index.html"), "<!doctype html>");
  fs.cpSync(path.join(root, "bin"), path.join(destination, "bin"), { recursive: true });
  fs.writeFileSync(
    path.join(source, "package.json"),
    JSON.stringify({ name: "@ait/desktop", version: "0.0.7" }),
  );
  for (const dependency of ["@oh-my-pi/pi-coding-agent", "@oven/bun-linux-x64", "bun"]) {
    fs.rmSync(path.join(source, "node_modules"), { recursive: true, force: true });
    const module = path.join(source, "node_modules", dependency);
    fs.mkdirSync(module, { recursive: true });
    fs.writeFileSync(path.join(module, "index.js"), "unwanted");
    execFileSync(process.execPath, [
      require.resolve("@electron/asar/bin/asar.js"),
      "pack",
      source,
      path.join(destination, "app.asar"),
    ]);
    uncache(path.join(destination, "app.asar"));
    assert.throws(
      () => verifyPackagedResources({ appOutDir, platform: "linux", version: "0.0.7" }),
      /OMP, its SDK, and Bun/,
    );
  }
});
