const fs = require("node:fs");
const path = require("node:path");

// Keep one pre-Chromium entrypoint for AppRun, desktop entries, updates, and tarballs.
exports.installLinuxLauncher = function installLinuxLauncher(appOutDir) {
  // Native packages install this tree as root. A private build umask (0077) or a
  // reused private output directory must not ship a root-only payload: every
  // directory and file needs read/traverse for the desktop user, executables keep
  // their execute bits, and chrome-sandbox keeps its installer-managed SUID mode.
  const normalize = (entry) => {
    const stat = fs.lstatSync(entry);
    if (stat.isSymbolicLink()) return;
    if (stat.isDirectory()) {
      fs.chmodSync(entry, stat.mode | 0o555);
      for (const name of fs.readdirSync(entry)) normalize(path.join(entry, name));
      return;
    }
    if (path.basename(entry) === "chrome-sandbox") return;
    const readable = stat.mode | 0o444;
    fs.chmodSync(entry, stat.mode & 0o111 ? readable | 0o111 : readable);
  };
  normalize(appOutDir);
  const launcher = path.join(appOutDir, "Ait");
  if (!fs.existsSync(`${launcher}.bin`)) {
    fs.renameSync(launcher, `${launcher}.bin`);
  }
  fs.copyFileSync(path.join(__dirname, "launcher.sh"), launcher);
  fs.chmodSync(launcher, 0o755);
};
