import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { resolveReleaseLayout } from "./release-layout.mjs";

function fixture(t, layout) {
  const root = mkdtempSync(path.join(tmpdir(), "ait-release-layout-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const binary = path.join(root, "bins", layout.binary);
  mkdirSync(binary, { recursive: true });
  writeFileSync(
    path.join(binary, "Cargo.toml"),
    `[package]\nname = "${layout.package}"\n\n[[bin]]\nname = "${layout.binary}"\n`,
  );
  mkdirSync(path.join(root, layout.desktop), { recursive: true });
  writeFileSync(path.join(root, layout.desktop, "package.json"), "{}");
  return root;
}

test("resolves renamed sources and immutable tags from before the rename", (t) => {
  for (const layout of [
    { package: "daemon", binary: "daemon", desktop: "apps/desktop" },
    { package: "server-bin", binary: "server", desktop: "apps/paseo" },
  ]) {
    assert.deepEqual(resolveReleaseLayout(fixture(t, layout)), layout);
  }
});

test("fails before building an inconsistent release source", (t) => {
  const root = fixture(t, { package: "daemon", binary: "daemon", desktop: "apps/desktop" });
  writeFileSync(path.join(root, "bins/daemon/Cargo.toml"), '[package]\nname = "other"\n');
  assert.throws(() => resolveReleaseLayout(root), assert.AssertionError);
});
