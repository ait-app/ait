import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");
const target = process.argv[2];
if (process.argv.length !== 3 || !["linux", "mac"].includes(target)) {
  throw new Error("Usage: node scripts/package-release.mjs linux|mac");
}
const expected = target === "mac" ? ["darwin", "arm64"] : ["linux", "x64"];
if (process.platform !== expected[0] || process.arch !== expected[1]) {
  throw new Error(
    `Build ${target} releases on ${expected.join("/")}, not ${process.platform}/${process.arch}.`,
  );
}
if (target === "mac") {
  if (!process.env.CSC_NAME && !process.env.CSC_LINK)
    throw new Error("macOS releases require CSC_NAME or CSC_LINK.");
  if (
    !process.env.APPLE_API_KEY &&
    !process.env.APPLE_APP_SPECIFIC_PASSWORD &&
    !process.env.APPLE_KEYCHAIN_PROFILE
  ) {
    throw new Error("macOS releases require Apple notarization credentials.");
  }
}
function run(command, args, cwd = desktop) {
  execFileSync(command, args, { cwd, stdio: "inherit" });
}
run(process.execPath, ["scripts/verify-release-version.mjs"], root);
run("npm", ["run", "build:desktop-assets"], root);
run(process.execPath, ["scripts/prepare-daemon.mjs"]);
run("npm", ["run", "build:main"]);
run("npm", [
  "exec",
  "--",
  "electron-builder",
  "--config",
  "electron-builder.yml",
  `--${target}`,
  `--${process.arch}`,
  "--publish",
  "never",
  ...(target === "mac" ? ["-c.forceCodeSigning=true"] : []),
]);
