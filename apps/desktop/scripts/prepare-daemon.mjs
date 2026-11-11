import { sourceVersion } from "../../../scripts/source-version.cjs";
import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import { execFileSync } from "node:child_process";

const desktop = fileURLToPath(new URL("..", import.meta.url));
const root = path.resolve(desktop, "../..");

export function stageDaemon({ binary, bridge, directory, version }) {
  for (const [name, executable] of [
    ["aitd", binary],
    ["ait-omp-acp", bridge],
  ]) {
    const actual = execFileSync(executable, ["--version"], { encoding: "utf8" }).trim();
    if (actual !== `${name} ${version}`) {
      throw new Error(`Expected ${name} ${version}, received ${actual}`);
    }
  }
  // This directory is generated packaging input. Never carry old sidecars forward.
  rmSync(directory, { recursive: true, force: true });
  mkdirSync(directory, { recursive: true });
  // Cargo builds `daemon`; packages ship it as `aitd` (ADR-212).
  for (const [name, executable] of [
    ["aitd", binary],
    ["ait-omp-acp", bridge],
  ]) {
    const staged = path.join(directory, name);
    copyFileSync(executable, staged);
    chmodSync(staged, 0o755);
  }
  return directory;
}

function main() {
  if (process.platform !== "darwin" && process.platform !== "linux") {
    throw new Error("Ait desktop releases support macOS and Linux only.");
  }
  const binary = process.env.AIT_SERVER_BIN || path.join(root, "target/release/daemon");
  const bridge = process.env.AIT_OMP_ACP_BIN || path.join(root, "target/release/ait-omp-acp");
  const targets = [];
  if (!process.env.AIT_SERVER_BIN) targets.push("-p", "daemon", "--bin", "daemon");
  if (!process.env.AIT_OMP_ACP_BIN) targets.push("-p", "ait-omp-acp", "--bin", "ait-omp-acp");
  if (targets.length) {
    execFileSync(
      "cargo",
      ["build", "--locked", "--release", "--target-dir", path.join(root, "target"), ...targets],
      { cwd: root, stdio: "inherit" },
    );
  }
  const pkg = JSON.parse(readFileSync(path.join(desktop, "package.json"), "utf8"));
  const version = sourceVersion(pkg.version, root);
  const staged = stageDaemon({
    binary: path.resolve(binary),
    bridge: path.resolve(bridge),
    directory: path.join(desktop, "release-resources/daemon"),
    version,
  });
  console.log(`Staged daemon and OMP ACP bridge ${version}: ${staged}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href)
  main();
