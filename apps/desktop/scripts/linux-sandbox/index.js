const fs = require("node:fs");
const path = require("node:path");

// Keep one pre-Chromium entrypoint for AppRun, desktop entries, updates, and tarballs.
exports.installLinuxLauncher = function installLinuxLauncher(appOutDir) {
  // Native packages install this directory as root, including reused private output directories.
  fs.chmodSync(appOutDir, 0o755);
  const launcher = path.join(appOutDir, "Ait");
  if (!fs.existsSync(`${launcher}.bin`)) {
    fs.renameSync(launcher, `${launcher}.bin`);
  }
  fs.copyFileSync(path.join(__dirname, "launcher.sh"), launcher);
  fs.chmodSync(launcher, 0o755);
};
