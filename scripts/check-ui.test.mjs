import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

function fixture(t, file, content) {
  const root = mkdtempSync(path.join(tmpdir(), "ait-ui-lint-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, "scripts"));
  copyFileSync(new URL("./check-ui.mjs", import.meta.url), path.join(root, "scripts/check-ui.mjs"));
  copyFileSync(new URL("../.oxfmtrc.json", import.meta.url), path.join(root, ".oxfmtrc.json"));
  symlinkSync(
    fileURLToPath(new URL("../node_modules", import.meta.url)),
    path.join(root, "node_modules"),
    "dir",
  );
  writeFileSync(path.join(root, ".gitignore"), "node_modules/\n");
  writeFileSync(path.join(root, "package.json"), '{"private":true}\n');
  const target = path.join(root, file);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, content);
  const git = (...args) => {
    const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git("init", "--quiet");
  git("add", ".");
  const commit = (message) =>
    git(
      "-c",
      "user.name=CI",
      "-c",
      "user.email=ci@example.invalid",
      "-c",
      "commit.gpgsign=false",
      "commit",
      "--quiet",
      "-m",
      message,
    );
  commit("baseline");
  const base = git("rev-parse", "HEAD");
  return {
    target,
    check: () => {
      git("add", file);
      commit("change");
      return spawnSync(process.execPath, ["scripts/check-ui.mjs", base], {
        cwd: root,
        encoding: "utf8",
      });
    },
  };
}

test("fixture-only changes pass without invoking the formatter with no targets", (t) => {
  const { target, check } = fixture(t, "scripts/fixtures/example.json", '{"version":1}\n');
  writeFileSync(target, '{"version":2}\n');
  const result = check();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /0 changed UI\/config files/);
});

test("changed source lint succeeds independently of formatting", (t) => {
  const { target, check } = fixture(t, "apps/example.ts", "export const answer = 1;\n");
  writeFileSync(target, "export const answer=42\n");
  const result = check();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /1 changed UI\/config files/);
});

test("invalid changed source still fails the UI lint check", (t) => {
  const { target, check } = fixture(t, "apps/example.ts", "export const answer = 1;\n");
  writeFileSync(target, "export const = ;\n");
  const result = check();
  assert.notEqual(result.status, 0, result.stdout + result.stderr);
});
