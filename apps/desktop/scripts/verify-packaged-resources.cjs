const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { extractFile, listPackage } = require("@electron/asar");
const { execFileSync } = require("node:child_process");
const { packagedVersion } = require("../../../scripts/source-version.cjs");

function verifyDaemonDirectory(resources, version) {
  const directory = path.join(resources, "bin");
  assert.deepEqual(
    fs.readdirSync(directory).sort(),
    ["ait-omp-acp", "aitd"],
    "Packaged resources/bin must contain only aitd and ait-omp-acp",
  );
  for (const name of ["aitd", "ait-omp-acp"]) {
    const binary = path.join(directory, name);
    assert(fs.lstatSync(binary).isFile(), `Bundled ${name} must be a regular file`);
    fs.accessSync(binary, fs.constants.X_OK);
    assert.equal(
      execFileSync(binary, ["--version"], { encoding: "utf8" }).trim(),
      `${name} ${version}`,
      `Bundled ${name} version differs from desktop`,
    );
  }
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
  }
  assert(
    fs.existsSync(path.join(resources, "app-dist", "index.html")),
    "Exported apps/mobile UI is missing",
  );
  const asar = path.join(resources, "app.asar");
  const pkg = JSON.parse(extractFile(asar, "package.json").toString());
  assert.equal(pkg.version, version, "Packaged desktop version differs from release version");
  assert.equal(
    packagedVersion(pkg.displayVersion ?? version),
    version,
    "Packaged display identity differs from desktop version",
  );
  assert.equal(pkg.name, "@ait/desktop", "Release must use Ait's independent package identity");
  if (platform !== "win32") verifyDaemonDirectory(resources, pkg.displayVersion ?? version);
  assert(
    !listPackage(asar).some((entry) =>
      /\/node_modules\/(?:@getpaseo|@ait)\/(?:cli|server|relay|plugin)(?:\/|$)/.test(entry),
    ),
    "Legacy Node server/CLI must not be bundled",
  );
  assert(
    !listPackage(asar).some((entry) =>
      /\/node_modules\/(?:@oh-my-pi\/|@oven\/bun|bun(?:\/|$))/.test(entry),
    ),
    "OMP, its SDK, and Bun must not be bundled",
  );
  console.log(
    `Verified Ait ${version}: apps/desktop, exported UI, ${platform === "win32" ? "no bundled daemon or bridge" : "resources/bin/aitd and ait-omp-acp only"}.`,
  );
}

module.exports = { verifyPackagedResources, verifyDaemonDirectory };
