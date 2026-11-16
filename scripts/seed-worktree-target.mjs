// Clones Cargo artifacts from the freshest compatible checkout into a new worktree.
// A missed seed only means a colder first build, so this never fails workspace setup.
import { spawnSync } from "node:child_process";
import {
  constants,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
} from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

// Only the dev profile is reused; release and cross-target output would just add clone time.
const SEEDED_ENTRIES = ["debug", "sherpa-onnx-prebuilt", "CACHEDIR.TAG"];
const RUST_INFO = ".rustc_info.json";

function git(root, args) {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout : "";
}

function pinnedRustVersion(root) {
  const toolchain = readFileSync(path.join(root, "rust-toolchain.toml"), "utf8");
  return /^channel\s*=\s*"([^"]+)"/m.exec(toolchain)?.[1] ?? null;
}

function describeCandidate(root) {
  try {
    const target = path.join(root, "target");
    const fingerprints = path.join(target, "debug", ".fingerprint");
    return {
      root,
      device: statSync(target).dev,
      // The parent's mtime ignores recompiles; each unit's directory is rewritten by them.
      freshness: readdirSync(fingerprints).reduce(
        (newest, unit) => Math.max(newest, statSync(path.join(fingerprints, unit)).mtimeMs),
        0,
      ),
      rustInfo: readFileSync(path.join(target, RUST_INFO), "utf8"),
    };
  } catch {
    return null;
  }
}

/** Picks the most recently built checkout that a clone can share blocks and artifacts with. */
export function chooseSeed(candidates, { device, rustVersion }) {
  return (
    candidates
      .filter(
        (candidate) =>
          candidate.device === device && candidate.rustInfo.includes(`rustc ${rustVersion} (`),
      )
      .toSorted((left, right) => right.freshness - left.freshness)[0] ?? null
  );
}

/** Names of the crates this checkout compiles from its own, branch-specific sources. */
export function workspaceCrates(worktree) {
  // A directory without a manifest, such as `crates/provider`, groups crates one level down.
  const packages = (parent, grouped) => {
    if (!existsSync(parent)) return [];
    return readdirSync(parent, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .flatMap((entry) => {
        const directory = path.join(parent, entry.name);
        const manifest = path.join(directory, "Cargo.toml");
        if (!existsSync(manifest)) return grouped ? [] : packages(directory, true);
        const name = /^name\s*=\s*"([^"]+)"/m.exec(readFileSync(manifest, "utf8"))?.[1];
        return name ? [name] : [];
      });
  };
  return ["crates", "bins"].flatMap((directory) => packages(path.join(worktree, directory), false));
}

// Cargo trusts a unit when its dep-info is newer than the sources. A clone can satisfy that with
// another branch's output, so workspace crates must rebuild; registry sources are immutable.
function forgetWorkspaceFingerprints(staging, crates) {
  const fingerprints = path.join(staging, "debug", ".fingerprint");
  const names = new Set(crates);
  for (const unit of readdirSync(fingerprints)) {
    const crate = /^(.+)-[0-9a-f]{16}$/.exec(unit)?.[1];
    if (names.has(crate)) rmSync(path.join(fingerprints, unit), { recursive: true });
  }
}

// `cp -c` silently degrades to a full copy without clone support, so probe first: a plain copy
// of a tens-of-gigabytes directory is worse than a cold build.
function copyWithCloneProbe(source, destination) {
  try {
    if (process.platform === "darwin") {
      // libuv reports ENOSYS for forced clones on macOS, so the volume type is the signal.
      const volume = spawnSync("df", ["-Y", path.dirname(destination)], { encoding: "utf8" });
      if (volume.status !== 0 || !/\sapfs\s/.test(volume.stdout)) return false;
      copyFileSync(source, destination, constants.COPYFILE_FICLONE);
    } else {
      copyFileSync(source, destination, constants.COPYFILE_FICLONE_FORCE);
    }
    return true;
  } catch {
    return false;
  }
}

function cloneEntry(source, destination) {
  const args =
    process.platform === "darwin"
      ? ["-c", "-R", source, destination]
      : ["-R", "--reflink=always", source, destination];
  return spawnSync("cp", args, { encoding: "utf8" });
}

export function seedWorktreeTarget(worktree) {
  const target = path.join(worktree, "target");
  if (existsSync(target)) return "target/ already exists; leaving it untouched.";
  const rustVersion = pinnedRustVersion(worktree);
  if (!rustVersion) return "rust-toolchain.toml pins no version; skipping the seed.";

  const candidates = git(worktree, ["worktree", "list", "--porcelain"])
    .split("\n")
    .filter((line) => line.startsWith("worktree "))
    .map((line) => line.slice("worktree ".length))
    .filter((root) => path.resolve(root) !== path.resolve(worktree))
    .map(describeCandidate)
    .filter(Boolean);
  const seed = chooseSeed(candidates, { device: statSync(worktree).dev, rustVersion });
  if (!seed)
    return `No checkout has a Rust ${rustVersion} build on this volume; skipping the seed.`;

  const staging = path.join(worktree, ".tmp", `target-seed-${process.pid}`);
  mkdirSync(staging, { recursive: true });
  try {
    const rustInfo = path.join(seed.root, "target", RUST_INFO);
    if (!copyWithCloneProbe(rustInfo, path.join(staging, RUST_INFO))) {
      return "This filesystem cannot clone files; skipping the seed.";
    }
    for (const entry of SEEDED_ENTRIES) {
      const source = path.join(seed.root, "target", entry);
      if (!existsSync(source)) continue;
      const clone = cloneEntry(source, path.join(staging, entry));
      if (clone.status !== 0) return `Could not clone ${entry}: ${clone.stderr.trim()}`;
    }
    forgetWorkspaceFingerprints(staging, workspaceCrates(worktree));
    // An Agent may have started a build meanwhile; its target/ wins over the seed.
    renameSync(staging, target);
    return `Seeded target/ from ${seed.root}.`;
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    console.log(seedWorktreeTarget(process.cwd()));
  } catch (error) {
    console.log(`Skipping the target/ seed: ${error.message}`);
  }
}
