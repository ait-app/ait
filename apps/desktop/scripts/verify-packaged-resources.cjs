const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { extractFile, listPackage } = require("@electron/asar");

function verifyDaemonDirectory(resources) {
  const directory = path.join(resources, "bin");
  assert.deepEqual(
    fs.readdirSync(directory).sort(),
    ["aitd"],
    "Packaged resources/bin must contain only aitd",
  );
  const daemon = path.join(directory, "aitd");
  assert(fs.lstatSync(daemon).isFile(), "Bundled daemon must be a regular file");
  fs.accessSync(daemon, fs.constants.X_OK);
}

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
  assert.match(manifest.bunPath, /^node_modules\/@oven\/bun-(darwin|linux|windows)-(x64|aarch64)(-musl)?\/bin\/bun(\.exe)?$/);
  if (platform) {
    const name = platform === "win32" ? "windows" : platform;
    assert(manifest.bunPath.startsWith(`node_modules/@oven/bun-${name}-`), "Native host target differs from package platform");
  }
  const root = fs.realpathSync(directory);
  // The patch marker proves the bundled SDK carries the Ait bridge; without it `/vibe` is
  // refused at runtime, so a package missing it must fail here instead of shipping silently.
  const hello = "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-hello.ts";
  const bridge = "node_modules/@oh-my-pi/pi-coding-agent/src/modes/rpc/ait-native-host-bridge.ts";
  for (const relative of [manifest.bunPath, manifest.entrypoint, hello, bridge, "notices/oh-my-pi-LICENSE", "notices/bun-upstream.txt", "notices/bun-linked-dependencies.txt", "notices/dependencies.json", "notices/NOTICE.txt"]) {
    const filename = path.join(directory, relative);
    assert(fs.existsSync(filename), `Missing native host asset: ${relative}`);
    assert(fs.statSync(filename).isFile(), `Native host asset is not a file: ${relative}`);
    assert(fs.realpathSync(filename).startsWith(`${root}${path.sep}`), `Native host asset escapes installation: ${relative}`);
  }
  if (platform !== "win32") fs.accessSync(path.join(directory, manifest.bunPath), fs.constants.X_OK);
  return manifest;
}

function verifyPackagedResources({ appOutDir, platform, version }) {
  assert(
    ["darwin", "linux", "win32"].includes(platform),
    `Unsupported release platform: ${platform}`,
  );
  const resources =
    platform === "darwin"
      ? path.join(appOutDir, "Ait.app", "Contents", "Resources")
      : path.join(appOutDir, "resources");
  if (platform === "win32") {
    assert(!fs.existsSync(path.join(resources, "bin")), "Windows must not bundle a daemon");
  } else {
    verifyDaemonDirectory(resources);
  }
  verifyNativeHostDirectory(path.join(resources, "provider-runtimes/omp-native-host/1"), platform);
  assert(
    fs.existsSync(path.join(resources, "app-dist", "index.html")),
    "Exported apps/mobile UI is missing",
  );
  const asar = path.join(resources, "app.asar");
  const pkg = JSON.parse(extractFile(asar, "package.json").toString());
  assert.equal(pkg.version, version, "Packaged desktop version differs from release version");
  assert.equal(pkg.name, "@ait/desktop", "Release must use Ait's independent package identity");
  assert(
    !listPackage(asar).some((entry) =>
      /\/node_modules\/(?:@getpaseo|@ait)\/(?:cli|server|relay|plugin)(?:\/|$)/.test(entry),
    ),
    "Legacy Node server/CLI must not be bundled",
  );
  console.log(
    `Verified Ait ${version}: apps/desktop, exported UI, ${platform === "win32" ? "no bundled daemon" : "resources/bin/aitd only"}.`,
  );
}

module.exports = { verifyPackagedResources, verifyDaemonDirectory, verifyNativeHostDirectory };
