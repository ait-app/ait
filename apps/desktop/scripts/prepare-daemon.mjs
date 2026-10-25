import { sourceVersion } from "../../../scripts/source-version.cjs";
import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import { execFileSync } from "node:child_process";

const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");

export function stageDaemon({ binary, directory, version }) {
  const actual = execFileSync(binary, ["--version"], { encoding: "utf8" }).trim();
  if (actual !== `daemon ${version}`) {
    throw new Error(`Expected daemon ${version}, received ${actual}`);
  }
  // This directory is generated packaging input. Never carry old sidecars forward.
  rmSync(directory, { recursive: true, force: true });
  mkdirSync(directory, { recursive: true });
  const staged = path.join(directory, "daemon");
  copyFileSync(binary, staged);
  chmodSync(staged, 0o755);
  return staged;
}

function main() {
  if (process.platform !== "darwin" && process.platform !== "linux") {
    throw new Error("Ait desktop releases support macOS and Linux only.");
  }
  const binary = process.env.AIT_SERVER_BIN || path.join(root, "target/release/daemon");
  if (!process.env.AIT_SERVER_BIN) {
    execFileSync(
      "cargo",
      [
        "build",
        "--locked",
        "--release",
        "--target-dir",
        path.join(root, "target"),
        "-p",
        "daemon",
        "--bin",
        "daemon",
      ],
      { cwd: root, stdio: "inherit" },
    );
  }
  const pkg = JSON.parse(readFileSync(path.join(desktop, "package.json"), "utf8"));
  const version = sourceVersion(pkg.version, root);
  const staged = stageDaemon({
    binary: path.resolve(binary),
    directory: path.join(desktop, "release-resources/daemon"),
    version,
  });
  console.log(`Staged daemon ${version}: ${staged}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  main();
