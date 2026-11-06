const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

// Shared by desktop packaging and standalone daemon archives so both accept exactly the same
// runtime layout: a bundle that passes one path cannot fail the other at release time.
function verifyNativeHostDirectory(directory, platform) {
  const manifestPath = path.join(directory, "manifest.json");
  assert(fs.statSync(manifestPath).size <= 4096, "Native host manifest exceeds 4096 bytes");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.adapter, "ait-omp-native-host");
  assert.equal(manifest.adapterVersion, 1);
  assert.equal(manifest.ompVersion, "18.17.0");
  assert.equal(manifest.bunVersion, "1.4.4");
  assert.equal(manifest.entrypoint, "node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts");
  assert.match(
    manifest.bunPath,
    /^node_modules\/@oven\/bun-(darwin|linux|windows)-(x64|aarch64)(-musl)?\/bin\/bun(\.exe)?$/,
  );
  if (platform) {
    const name = platform === "win32" ? "windows" : platform;
    assert(
      manifest.bunPath.startsWith(`node_modules/@oven/bun-${name}-`),
      "Native host target differs from package platform",
    );
  }
  const root = fs.realpathSync(directory);
  // The patch marker proves the bundled SDK carries the Ait bridge; without it `/vibe` is
  // refused at runtime, so a package missing it must fail here instead of shipping silently.
  const hello = "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-hello.ts";
  const bridge = "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-bridge.ts";
  for (const relative of [
    manifest.bunPath,
    manifest.entrypoint,
    hello,
    bridge,
    "notices/oh-my-pi-LICENSE",
    "notices/bun-upstream.txt",
    "notices/bun-linked-dependencies.txt",
    "notices/dependencies.json",
    "notices/NOTICE.txt",
  ]) {
    const filename = path.join(directory, relative);
    assert(fs.existsSync(filename), `Missing native host asset: ${relative}`);
    assert(fs.statSync(filename).isFile(), `Native host asset is not a file: ${relative}`);
    assert(
      fs.realpathSync(filename).startsWith(`${root}${path.sep}`),
      `Native host asset escapes installation: ${relative}`,
    );
  }
  if (platform !== "win32")
    fs.accessSync(path.join(directory, manifest.bunPath), fs.constants.X_OK);
  return manifest;
}

module.exports = { verifyNativeHostDirectory };
