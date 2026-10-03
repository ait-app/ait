import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

// The launcher reads Linux /proc state as well as POSIX command interfaces.
const it = test.runIf(process.platform === "linux");

const require = createRequire(import.meta.url);
const afterPack = require("../../scripts/after-pack.js").default;
const { version } = require("../../package.json");

beforeEach(() => vi.stubEnv("AIT_DESKTOP_SMOKE", "0"));
afterEach(() => vi.unstubAllEnvs());

function createPackagedApp(root: string) {
  const app = join(root, "app with spaces");
  const resources = join(app, "resources");
  const source = join(root, "asar-source");
  mkdirSync(join(resources, "bin"), { recursive: true });
  mkdirSync(join(resources, "app-dist"));
  mkdirSync(source);
  writeFileSync(join(source, "package.json"), JSON.stringify({ name: "@ait/desktop", version }));
  // The ASAR 3 API resolves before its output stream finishes; CLI exit waits for pending writes.
  const archive = spawnSync(
    process.execPath,
    [require.resolve("@electron/asar/bin/asar.js"), "pack", source, join(resources, "app.asar")],
    { encoding: "utf8" },
  );
  expect(archive.status, archive.stderr).toBe(0);
  writeFileSync(join(resources, "app-dist", "index.html"), "<!doctype html><title>Ait</title>");
  writeFileSync(join(resources, "bin", "daemon"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  writeFileSync(
    join(app, "Ait"),
    `#!${process.execPath}\nconsole.log(JSON.stringify(process.argv.slice(2)));\n`,
    { mode: 0o755 },
  );
  writeFileSync(join(app, "chrome-sandbox"), "helper", { mode: 0o755 });
  return {
    appOutDir: app,
    electronPlatformName: "linux",
    arch: 1,
    packager: { appInfo: { version } },
  };
}

test("validates Linux package resources before installing the launcher on any host", async () => {
  const root = mkdtempSync(join(tmpdir(), "ait-linux-after-pack-"));
  try {
    const context = createPackagedApp(root);
    const executable = join(context.appOutDir, "Ait");
    const original = readFileSync(executable);
    const legacyBin = join(context.appOutDir, "resources", "bin", "ait-worker");
    writeFileSync(legacyBin, "obsolete sidecar");
    await expect(afterPack(context)).rejects.toThrow("must contain only daemon");
    expect(existsSync(`${executable}.bin`)).toBe(false);
    expect(readFileSync(executable)).toEqual(original);

    rmSync(legacyBin);
    await afterPack(context);
    expect(readFileSync(`${executable}.bin`)).toEqual(original);
    expect(readFileSync(executable, "utf8")).toContain("AIT_DESKTOP_SANDBOX_REASON");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

async function launch(
  options: {
    namespaces?: boolean;
    helper?: string;
    mount?: string;
    env?: NodeJS.ProcessEnv;
    args?: string[];
    symlink?: boolean;
    rerun?: boolean;
  } = {},
) {
  const root = mkdtempSync(join(tmpdir(), "paseo-launcher-"));
  try {
    const context = createPackagedApp(root);
    const app = context.appOutDir;
    const commands = join(root, "commands");
    mkdirSync(commands);
    // The command interface represents the host's userns policy, independent of CI's host.
    writeFileSync(join(commands, "unshare"), `#!/bin/sh\nexit ${options.namespaces ? 0 : 1}\n`);
    chmodSync(join(commands, "unshare"), 0o755);
    for (const [name, output] of Object.entries({
      stat: options.helper ?? "1000:755",
      findmnt: options.mount ?? "rw",
    })) {
      writeFileSync(join(commands, name), `#!/bin/sh\nprintf '%s\\n' '${output}'\n`);
      chmodSync(join(commands, name), 0o755);
    }
    await afterPack(context);
    if (options.rerun) await afterPack(context);
    const executablePath = options.symlink ? join(root, "paseo") : join(app, "Ait");
    if (options.symlink) symlinkSync(join(app, "Ait"), executablePath);
    const args = options.args ?? ["path with spaces", "$(touch never)", "semi;colon", "*.txt"];
    const result = spawnSync(executablePath, args, {
      encoding: "utf8",
      env: {
        ...process.env,
        FORCE_COLOR: undefined,
        PATH: `${commands}:${process.env.PATH}`,
        APPIMAGE: "/tmp/Ait.AppImage",
        AIT_DESKTOP_SMOKE: "0",
        ...options.env,
      },
    });
    expect(result.status, result.stderr).toBe(0);
    return { args: JSON.parse(result.stdout), stderr: result.stderr, input: args };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

it("launches a portable app with the fallback on argv when user namespaces are denied", async () => {
  const result = await launch();
  expect(result.args).toEqual(["--no-sandbox", ...result.input]);
  expect(result.stderr).toContain("[linux-sandbox] disabled");
});

it("keeps sandboxing for AppImage when the real user can create namespaces", async () => {
  const result = await launch({ namespaces: true });
  expect(result.args).toEqual(result.input);
  expect(result.stderr).toContain("[linux-sandbox] enabled: user namespaces available");
});

it("keeps sandboxing via the installed root-owned helper when userns is denied", async () => {
  const result = await launch({ helper: "0:4755", env: { APPIMAGE: "" } });
  expect(result.args).toEqual(result.input);
  expect(result.stderr).toContain("[linux-sandbox] enabled: root-owned SUID helper available");
});

it("does not trust 4755 on a nosuid mount", async () => {
  const result = await launch({ helper: "0:4755", mount: "rw,nosuid,nodev" });
  expect(result.args).toEqual(["--no-sandbox", ...result.input]);
});

it("preserves sandbox flags and all Node entrypoint arguments without probing", async () => {
  const result = await launch({ env: { ELECTRON_RUN_AS_NODE: "1" }, args: ["--version"] });
  expect(result.args).toEqual(["--version"]);
  expect(result.stderr).toBe("");
});

it("reports an explicit user override without injecting a duplicate", async () => {
  const result = await launch({ namespaces: true, args: ["--no-sandbox", "--version"] });
  expect(result.args).toEqual(result.input);
  expect(result.stderr).toContain("[linux-sandbox] disabled: requested by --no-sandbox");
});

it("resolves symlink launches and keeps the real executable intact on repeated packaging", async () => {
  const result = await launch({ symlink: true, rerun: true });
  expect(result.args).toEqual(["--no-sandbox", ...result.input]);
});

it("does not depend on APPIMAGE being present for an extracted portable app", async () => {
  const result = await launch({ env: { APPIMAGE: "" } });
  expect(result.args).toEqual(["--no-sandbox", ...result.input]);
});

it("applies a debugging environment sandbox override before Chromium starts", async () => {
  const result = await launch({
    namespaces: true,
    env: { AIT_ELECTRON_FLAGS: "--disable-gpu\t--no-sandbox" },
  });
  expect(result.args).toEqual(["--no-sandbox", ...result.input]);
  expect(result.stderr).toContain("requested by AIT_ELECTRON_FLAGS");
});
