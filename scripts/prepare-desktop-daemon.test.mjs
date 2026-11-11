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
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

function fixture(t, version, name = "aitd") {
  const root = mkdtempSync(path.join(tmpdir(), "ait-stage-daemon-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const binary = path.join(root, "compiled-daemon");
  writeFileSync(binary, `#!/bin/sh\nprintf '${name} ${version}\\n'\n`);
  chmodSync(binary, 0o755);
  const bridge = path.join(root, "ait-omp-acp");
  writeFileSync(bridge, "#!/bin/sh\nprintf 'ait-omp-acp 0.0.7\\n'\n");
  chmodSync(bridge, 0o755);
  return { binary, bridge, directory: path.join(root, "staged"), version: "0.0.7" };
}

test("staging removes stale sidecars and copies the matching native pair", (t) => {
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
  assert.deepEqual(readdirSync(input.directory).sort(), ["ait-omp-acp", "aitd"]);
  assert.deepEqual(readFileSync(path.join(staged, "aitd")), readFileSync(input.binary));
  assert.deepEqual(readFileSync(path.join(staged, "ait-omp-acp")), readFileSync(input.bridge));
});

test("staging rejects an old daemon even when AIT_SERVER_BIN points to it", (t) => {
  const input = fixture(t, "0.0.6");
  assert.throws(() => stageDaemon(input), /Expected aitd 0\.0\.7/);
  assert(!existsSync(input.directory));
});

test("staging rejects the old server binary even when its version matches", (t) => {
  const input = fixture(t, "0.0.7", "server");
  assert.throws(() => stageDaemon(input), /Expected aitd 0\.0\.7, received server 0\.0\.7/);
  assert(!existsSync(input.directory));
});

test("staging rejects a missing or mismatched bridge before replacing staged resources", (t) => {
  for (const reported of [null, "ait-omp-acp 0.0.6", "omp 0.0.7"]) {
    const input = fixture(t, "0.0.7");
    if (reported === null) rmSync(input.bridge);
    else writeFileSync(input.bridge, `#!/bin/sh\necho '${reported}'\n`);
    mkdirSync(input.directory);
    writeFileSync(path.join(input.directory, "sentinel"), "keep");
    assert.throws(() => stageDaemon(input));
    assert.deepEqual(readdirSync(input.directory), ["sentinel"]);
  }
});

test("staging uses the same source identity for daemon and bridge", (t) => {
  const version = "git-2026-11-11-1234abcd";
  const input = fixture(t, version);
  input.version = version;
  writeFileSync(input.bridge, `#!/bin/sh\necho 'ait-omp-acp ${version}'\n`);
  assert.doesNotThrow(() => stageDaemon(input));
});

test("the Linux baseline gate checks the bridge even when it needs no libstdc++ symbols", (t) => {
  const input = fixture(t, "0.0.7");
  const root = path.dirname(input.binary);
  const commands = path.join(root, "commands");
  const output = path.join(root, "target/x86_64-unknown-linux-gnu/release");
  mkdirSync(commands);
  mkdirSync(output, { recursive: true });
  mkdirSync(path.join(root, "bins/omp-acp"), { recursive: true });
  writeFileSync(path.join(root, "bins/omp-acp/Cargo.toml"), '[package]\nname = "ait-omp-acp"\n');
  writeFileSync(path.join(output, "daemon"), "GLIBC_2.28 GLIBCXX_3.4.25");
  writeFileSync(path.join(output, "ait-omp-acp"), "GLIBC_2.28");
  for (const [name, script] of [
    ["docker", "#!/bin/sh\nexit 0\n"],
    ["objdump", '#!/bin/sh\ncat "$2"\n'],
  ]) {
    writeFileSync(path.join(commands, name), script, { mode: 0o755 });
  }
  const run = () =>
    spawnSync(
      "bash",
      [
        fileURLToPath(new URL("./build-linux-daemon.sh", import.meta.url)),
        "daemon",
        "daemon",
        "x86_64-unknown-linux-gnu",
        "1.98.1",
      ],
      {
        cwd: root,
        env: { ...process.env, PATH: `${commands}:${process.env.PATH}` },
        encoding: "utf8",
      },
    );
  const accepted = run();
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /ait-omp-acp requires GLIBC_2\.28 and GLIBCXX_none/);
  writeFileSync(path.join(output, "ait-omp-acp"), "GLIBC_2.39");
  const incompatible = run();
  assert.notEqual(incompatible.status, 0);
  assert.match(incompatible.stderr, /exceeds the 2\.28 baseline/);
  rmSync(path.join(output, "ait-omp-acp"));
  assert.notEqual(run().status, 0, "a missing current bridge cannot bypass the baseline check");
});
