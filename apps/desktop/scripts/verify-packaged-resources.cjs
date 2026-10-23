const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { extractFile, listPackage } = require("@electron/asar");

function verifyDaemonDirectory(resources) {
  const directory = path.join(resources, "bin");
  assert.deepEqual(
    fs.readdirSync(directory).sort(),
    ["daemon"],
    "Packaged resources/bin must contain only daemon",
  );
  const daemon = path.join(directory, "daemon");
  assert(fs.lstatSync(daemon).isFile(), "Bundled daemon must be a regular file");
  fs.accessSync(daemon, fs.constants.X_OK);
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
    `Verified Ait ${version}: apps/desktop, exported UI, ${platform === "win32" ? "no bundled daemon" : "resources/bin/daemon only"}.`,
  );
}

module.exports = { verifyPackagedResources, verifyDaemonDirectory };
