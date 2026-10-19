// Observe an installed deb or rpm without repairing its sandbox permissions.
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync, spawn } = require("node:child_process");

const APP_PATH = "/opt/Ait";

// The launcher reports its sandbox decision on stderr before it execs Electron.
function sandboxDecision(launcher) {
  return new Promise((resolve, reject) => {
    const child = spawn(launcher, ["--version"], {
      stdio: ["ignore", "ignore", "pipe"],
    });
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`Launcher printed no sandbox decision: ${stderr}`));
    }, 20_000);
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
      const line = stderr.split("\n").find((entry) => entry.startsWith("[linux-sandbox]"));
      if (!line) return;
      clearTimeout(timer);
      child.kill("SIGKILL");
      resolve(line);
    });
    child.on("error", reject);
  });
}

async function main() {
  if (process.getuid() === 0) throw new Error("Launch the smoke as an unprivileged user");
  const helper = fs.statSync(path.join(APP_PATH, "chrome-sandbox"));
  if (helper.uid !== 0 || (helper.mode & 0o7777) !== 0o4755) {
    throw new Error("Installed native package did not provide a root-owned 4755 helper");
  }
  const decision = await sandboxDecision(path.join(APP_PATH, "Ait"));
  console.log(decision);
  if (!decision.startsWith("[linux-sandbox] enabled:")) {
    throw new Error(`Installed native package launched without a sandbox: ${decision}`);
  }
  execFileSync("xvfb-run", ["-a", process.execPath, path.join(__dirname, "rust-startup.e2e.mjs")], {
    stdio: "inherit",
    env: { ...process.env, AIT_PACKAGED_APP: APP_PATH },
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
