import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
if (process.argv.length !== 2) {
  throw new Error("Usage: npm run build:windows");
}
const npm = process.env.npm_execpath;
if (!npm) throw new Error("Run this build through npm run build:windows.");
const crossBuild = process.platform !== "win32";

function run(args, cwd = desktop) {
  execFileSync(process.execPath, [npm, ...args], { cwd, stdio: "inherit" });
}

run(["run", "verify:release"], root);
run(["run", "build:desktop-assets"], root);
run(["run", "build:main"]);
run([
  "exec",
  "--",
  "electron-builder",
  "--config",
  "electron-builder.yml",
  "--win",
  ...(crossBuild ? ["zip", "-c.win.signAndEditExecutable=false"] : []),
  "--x64",
  "--publish",
  "never",
]);
