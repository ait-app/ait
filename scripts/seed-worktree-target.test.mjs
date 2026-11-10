import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { chooseSeed, seedWorktreeTarget, workspaceCrates } from "./seed-worktree-target.mjs";

const rust = "rustc 1.98.1 (48a229cea 2026-09-01)";
const candidate = (root, freshness, overrides = {}) => ({
  root,
  freshness,
  device: 1,
  rustInfo: rust,
  ...overrides,
});
const wanted = { device: 1, rustVersion: "1.98.1" };

test("chooses the most recently built checkout", () => {
  assert.equal(chooseSeed([candidate("old", 1), candidate("new", 2)], wanted).root, "new");
});

test("rejects other toolchains, since cargo would rebuild everything anyway", () => {
  const stale = candidate("stale", 9, { rustInfo: "rustc 1.90.0 (aaaaaaaaa 2026-01-01)" });
  assert.equal(chooseSeed([stale, candidate("match", 1)], wanted).root, "match");
});

test("rejects other volumes, where a clone would be a full copy", () => {
  assert.equal(chooseSeed([candidate("usb", 9, { device: 2 })], wanted), null);
});

function repository(t) {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), "ait-seed-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const main = path.join(root, "main");
  mkdirSync(main);
  const git = (...args) => {
    const result = spawnSync("git", args, { cwd: main, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
  };
  git("init", "--quiet");
  writeFileSync(path.join(main, "rust-toolchain.toml"), 'channel = "1.98.1"\n');
  writeFileSync(path.join(main, ".gitignore"), "target/\n.tmp/\n");
  git("add", ".");
  git(
    "-c",
    "user.name=CI",
    "-c",
    "user.email=ci@example.invalid",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "-qm",
    "init",
  );
  const worktree = path.join(root, "feature");
  git("worktree", "add", "--quiet", "-b", "feature", worktree);
  return { main, worktree };
}

function build(root) {
  mkdirSync(path.join(root, "target/debug/.fingerprint"), { recursive: true });
  writeFileSync(path.join(root, "target/debug/deps.rlib"), "artifact");
  for (const unit of ["domain-0123456789abcdef", "domain-extra-0123456789abcdef"]) {
    mkdirSync(path.join(root, "target/debug/.fingerprint", unit));
  }
  writeFileSync(path.join(root, "target/.rustc_info.json"), rust);
  mkdirSync(path.join(root, "target/release"));
}

test("leaves an existing target directory alone", (t) => {
  const { main, worktree } = repository(t);
  build(main);
  mkdirSync(path.join(worktree, "target"));
  assert.match(seedWorktreeTarget(worktree), /already exists/);
  assert.equal(existsSync(path.join(worktree, "target/debug")), false);
});

test("skips quietly when no checkout has been built", (t) => {
  const { worktree } = repository(t);
  assert.match(seedWorktreeTarget(worktree), /skipping the seed/);
  assert.equal(existsSync(path.join(worktree, "target")), false);
});

test(
  "clones the dev profile only and leaves no staging directory",
  { skip: process.platform !== "darwin" },
  (t) => {
    const { main, worktree } = repository(t);
    build(main);
    assert.match(seedWorktreeTarget(worktree), /Seeded target\//);
    assert.equal(readFileSync(path.join(worktree, "target/debug/deps.rlib"), "utf8"), "artifact");
    assert.equal(existsSync(path.join(worktree, "target/release")), false);
    assert.equal(readFileSync(path.join(worktree, "target/.rustc_info.json"), "utf8"), rust);
    assert.deepEqual(
      spawnSync("ls", [path.join(worktree, ".tmp")], { encoding: "utf8" }).stdout,
      "",
    );
  },
);

test(
  "drops workspace fingerprints so another branch's output can never look fresh",
  { skip: process.platform !== "darwin" },
  (t) => {
    const { main, worktree } = repository(t);
    build(main);
    mkdirSync(path.join(worktree, "crates/domain"), { recursive: true });
    writeFileSync(path.join(worktree, "crates/domain/Cargo.toml"), 'name = "domain"\n');
    assert.deepEqual(workspaceCrates(worktree), ["domain"]);
    assert.match(seedWorktreeTarget(worktree), /Seeded target\//);
    const fingerprints = path.join(worktree, "target/debug/.fingerprint");
    assert.equal(existsSync(path.join(fingerprints, "domain-0123456789abcdef")), false);
    // A registry crate whose name merely starts with a workspace crate's name stays reusable.
    assert.equal(existsSync(path.join(fingerprints, "domain-extra-0123456789abcdef")), true);
  },
);
