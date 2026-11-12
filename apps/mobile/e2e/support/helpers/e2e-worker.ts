import { execFileSync } from "node:child_process";

import { chmod, copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { repoRoot } from "./ait-daemon";
import { startIsolatedHostDaemon } from "./isolated-host-daemon";

export interface E2EWorker {
  close(): Promise<void>;
}

export interface E2EWorkerOptions {
  forkProviders?: string[];
  injectPaseoTools?: boolean;
  daemonConfig?: Record<string, unknown>;
  environment?: Record<string, string>;
}

function resolveOptionalHome(value: string | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed) return null;
  return path.resolve(trimmed.startsWith("~/") ? path.join(homedir(), trimmed.slice(2)) : trimmed);
}

async function createFakeEditorBin(): Promise<string> {
  const binDir = await mkdtemp(path.join(tmpdir(), "ait-e2e-editor-bin-"));
  let realGhPath = "";
  try {
    const locator = process.platform === "win32" ? "where.exe" : "which";
    const candidates = execFileSync(locator, ["gh"], { encoding: "utf8" })
      .split(/\r?\n/u)
      .map((candidate) => candidate.trim())
      .filter(Boolean);
    realGhPath =
      candidates.find(
        (candidate) =>
          process.platform !== "win32" || !/\.(?:cmd|bat)$/iu.test(path.extname(candidate)),
      ) ?? "";
  } catch {
    // The local GitHub fixture remains usable without a system gh binary.
  }
  const fakeEditorSource = `#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const recordPath = process.env.PASEO_E2E_EDITOR_RECORD_PATH;
if (recordPath) {
  fs.appendFileSync(recordPath, JSON.stringify({
    command: path.basename(process.argv[1]),
    args: process.argv.slice(2),
    cwd: process.cwd(),
    at: Date.now()
  }) + "\\n");
}
`;
  for (const editorCommand of ["cursor", "code"]) {
    const editorPath = path.join(binDir, editorCommand);
    await writeFile(editorPath, fakeEditorSource);
    await chmod(editorPath, 0o755);
    if (process.platform === "win32") {
      await writeFile(`${editorPath}.cmd`, `@node "%~dp0${editorCommand}" %*\r\n`);
    }
  }

  const fakeGhPath = path.join(binDir, "gh");
  const fakeGhSource = `#!/usr/bin/env node
const { spawnSync } = require("child_process");
const args = process.argv.slice(2);
const fixtureRemote = "https://github.com/paseo-e2e/local-fixture.git";
const origin = spawnSync("git", ["config", "--get", "remote.origin.url"], {
  encoding: "utf8",
  stdio: ["ignore", "pipe", "ignore"]
}).stdout?.trim();

if (origin === fixtureRemote) {
  const command = args.slice(0, 2).join(" ");
  if (command === "auth status") process.exit(0);
  if (command === "repo view") {
    process.stdout.write(JSON.stringify({ owner: { login: "paseo-e2e" }, name: "local-fixture", parent: null }));
    process.exit(0);
  }
  if (command === "issue list") {
    process.stdout.write("[]");
    process.exit(0);
  }
  if (command === "pr list" || command === "pr view") {
    const isFork = args.includes("2");
    const pr = {
      number: isFork ? 2 : 1,
      title: "Use pasted PR as start ref",
      url: "https://github.com/paseo-e2e/local-fixture/pull/" + (isFork ? 2 : 1),
      state: "OPEN",
      body: null,
      labels: [],
      baseRefName: "main",
      headRefName: isFork ? "pr-branch-2" : "pr-branch-1",
      updatedAt: "2026-01-01T00:00:00Z"
    };
    process.stdout.write(JSON.stringify(command === "pr list" ? [pr] : pr));
    process.exit(0);
  }
  if (command === "api graphql" && args.some((arg) => arg.includes("PullRequestCheckoutTarget"))) {
    const isFork = args.some((arg) => arg === "number=2");
    process.stdout.write(JSON.stringify({
      data: { repository: { pullRequest: {
        number: isFork ? 2 : 1,
        baseRefName: "main",
        headRefName: isFork ? "pr-branch-2" : "pr-branch-1",
        isCrossRepository: isFork,
        headRepositoryOwner: { login: isFork ? "fork-owner" : "paseo-e2e" },
        headRepository: {
          sshUrl: isFork ? "git@github.com:fork-owner/local-fixture.git" : "git@github.com:paseo-e2e/local-fixture.git",
          url: isFork ? "https://github.com/fork-owner/local-fixture" : fixtureRemote
        }
      } } }
    }));
    process.exit(0);
  }
  process.stderr.write("Unsupported local GitHub fixture command: " + args.join(" ") + "\\n");
  process.exit(1);
}

const realGhPath = ${JSON.stringify(realGhPath)};
if (!realGhPath) process.exit(127);
const result = spawnSync(realGhPath, args, { stdio: "inherit" });
process.exit(result.status ?? 1);
`;
  await writeFile(fakeGhPath, fakeGhSource);
  await chmod(fakeGhPath, 0o755);
  if (process.platform === "win32") {
    await writeFile(`${fakeGhPath}.cmd`, '@node "%~dp0gh" %*\r\n');
  }
  return binDir;
}

export async function startE2EWorker(
  workerIndex: number,
  options: E2EWorkerOptions = {},
): Promise<E2EWorker> {
  const requestedRoot = resolveOptionalHome(process.env.E2E_AIT_DATA_ROOT);
  const dataDir = requestedRoot
    ? path.join(requestedRoot, `worker-${workerIndex}`)
    : await mkdtemp(path.join(tmpdir(), `ait-e2e-worker-${workerIndex}-`));
  const preserveHome = Boolean(requestedRoot) || process.env.E2E_KEEP_AIT_DATA === "1";
  const fakeEditorBin = await createFakeEditorBin();
  const editorRecordPath = path.join(dataDir, "editor-open-records.jsonl");
  const serverId = `srv_e2e_worker_${workerIndex}`;
  const codexFixture = path.join(fakeEditorBin, "codex-offline");

  try {
    if (
      process.env.E2E_FORK_PASEO_HOME_FROM ||
      options.forkProviders?.length ||
      options.injectPaseoTools
    ) {
      throw new Error("Paseo metadata/provider/MCP fixtures are not supported by Ait E2E");
    }
    await mkdir(dataDir, { recursive: true });
    await copyFile(
      path.join(repoRoot, "crates/provider-codex/tests/fixtures/codex_app_server.py"),
      codexFixture,
    );
    await chmod(codexFixture, 0o755);
    // Worker-scoped fixture config lets a spec exercise provider discovery without
    // reading the developer's provider state or sharing configuration with other specs.
    if (options.daemonConfig) {
      await writeFile(
        path.join(dataDir, "config.json"),
        `${JSON.stringify(options.daemonConfig, null, 2)}\n`,
      );
    }
    const daemon = await startIsolatedHostDaemon(serverId, {
      dataDir,
      preserveHome,
      environment: {
        NODE_ENV: "development",
        AIT_SERVER_CODEX_BIN: codexFixture,
        ...(process.env.E2E_REAL_PROVIDERS === "1"
          ? {
              AIT_SERVER_CODEX_BIN: process.env.AIT_SERVER_CODEX_BIN ?? "codex",
              AIT_SERVER_CLAUDE_BIN: process.env.AIT_SERVER_CLAUDE_BIN ?? "claude",
            }
          : {}),
        PATH: `${fakeEditorBin}${path.delimiter}${process.env.PATH ?? ""}`,
        PASEO_E2E_EDITOR_RECORD_PATH: editorRecordPath,
        ...options.environment,
      },
    });

    process.env.E2E_DAEMON_PORT = String(daemon.port);
    process.env.E2E_SERVER_ID = daemon.serverId;
    process.env.E2E_AIT_DATA_DIR = daemon.dataDir;
    process.env.E2E_EDITOR_RECORD_PATH = editorRecordPath;
    delete process.env.E2E_RELAY_PORT;
    delete process.env.E2E_RELAY_DAEMON_PUBLIC_KEY;

    console.log(
      `[e2e] Worker ${workerIndex} daemon started on port ${daemon.port}, home: ${daemon.dataDir}`,
    );
    return {
      close: async () => {
        await daemon.close();
        await rm(fakeEditorBin, { recursive: true, force: true });
        console.log(`[e2e] Worker ${workerIndex} daemon stopped`);
      },
    };
  } catch (error) {
    await rm(fakeEditorBin, { recursive: true, force: true });
    if (!preserveHome) await rm(dataDir, { recursive: true, force: true });
    throw error;
  }
}
