#!/usr/bin/env node
// Exports the hosted browser client (ADR-188) for the static image in web-console/.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const app = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(app, "../..");
const output = path.join(app, "web-console/dist");
const expo = createRequire(path.join(app, "package.json")).resolve("expo/bin/cli");
const env = {
  ...process.env,
  // Cold transforms of the generated protocol validator exceed Node's default
  // heap. Keep one worker so its 4 GiB budget fits a CI runner.
  NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ""} --max-old-space-size=4096`.trim(),
  AIT_WEB_PLATFORM: "web",
  EXPO_PUBLIC_AIT_CONSOLE: "1",
  EXPO_NO_DOTENV: "1",
  EXPO_NO_TELEMETRY: "1",
};
// The client owns the site root; a sub-path build would break deep links and the login return.
delete env.AIT_WEB_BASE_PATH;

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, env, stdio: "inherit" });
}

run("npm", ["run", "build:ui-deps"], root);
rmSync(output, { recursive: true, force: true });
// --clear: the Metro cache is shared with desktop exports, which inline a different
// EXPO_PUBLIC_AIT_CONSOLE value.
run(
  process.execPath,
  [expo, "export", "--platform", "web", "--output-dir", output, "--max-workers", "1", "--clear"],
  app,
);
if (!existsSync(path.join(output, "index.html")))
  throw new Error("Web client export is missing index.html");

const { version } = JSON.parse(readFileSync(path.join(app, "package.json"), "utf8"));
const commit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
writeFileSync(
  path.join(output, "client-build.json"),
  `${JSON.stringify({ version, commit }, null, 2)}\n`,
);
console.log(`AIT web client ${version} (${commit}) exported to ${output}`);
