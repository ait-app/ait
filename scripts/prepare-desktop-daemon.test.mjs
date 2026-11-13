import assert from "node:assert/strict";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { stageDaemon } from "../apps/desktop/scripts/prepare-daemon.mjs";

function fixture(t, version, name = "aitd") {
  const root = mkdtempSync(path.join(tmpdir(), "ait-stage-daemon-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const binary = path.join(root, "compiled-daemon");
  writeFileSync(binary, `#!/bin/sh\nprintf '${name} ${version}\\n'\n`);
  chmodSync(binary, 0o755);
  return { binary, directory: path.join(root, "staged"), version: "0.0.7" };
}

test("staging removes stale sidecars and copies only the matching daemon", (t) => {
  const input = fixture(t, "0.0.7");
  mkdirSync(input.directory);
  for (const name of [
    "daemon",
    "ait-daemon",
    "ait-worker",
    "paseo",
    "server",
    "server.exe",
    "daemon.exe",
  ])
    writeFileSync(path.join(input.directory, name), "stale");
  const staged = stageDaemon(input);
  assert.deepEqual(readdirSync(input.directory), ["aitd"]);
  assert.deepEqual(readFileSync(staged), readFileSync(input.binary));
});

test("staging rejects an old daemon even when AITD_BIN points to it", (t) => {
  const input = fixture(t, "0.0.6");
  assert.throws(() => stageDaemon(input), /Expected aitd 0\.0\.7/);
  assert(!existsSync(input.directory));
});

test("staging rejects the old server binary even when its version matches", (t) => {
  const input = fixture(t, "0.0.7", "server");
  assert.throws(() => stageDaemon(input), /Expected aitd 0\.0\.7, received server 0\.0\.7/);
  assert(!existsSync(input.directory));
});
