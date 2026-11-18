import { execFile } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { test as base } from "../fixtures";
import { repoRoot } from "./ait-daemon";

async function buildPeer(): Promise<string> {
  const { stdout } = await promisify(execFile)(
    "cargo",
    [
      "build",
      "--locked",
      "-p",
      "provider-omp",
      "--example",
      "omp-test-peer",
      "--message-format=json",
    ],
    { cwd: repoRoot, maxBuffer: 8 * 1024 * 1024 },
  );
  for (const line of stdout.trim().split("\n")) {
    const artifact = JSON.parse(line);
    if (
      artifact.reason === "compiler-artifact" &&
      artifact.target.name === "omp-test-peer" &&
      artifact.executable
    ) {
      return artifact.executable;
    }
  }
  throw new Error("Cargo did not report an executable for the OMP test peer");
}

export const test = base.extend<{}, { ompPeer: string; ompAgentDirectory: string }>({
  ompPeer: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright fixture dependency declaration.
    async ({}, use) => {
      await use(await buildPeer());
    },
    { scope: "worker", timeout: 120_000 },
  ],
  ompAgentDirectory: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright fixture dependency declaration.
    async ({}, use) => {
      const directory = await mkdtemp(path.join(tmpdir(), "ait-e2e-omp-"));
      try {
        await use(directory);
      } finally {
        await rm(directory, { recursive: true, force: true });
      }
    },
    { scope: "worker" },
  ],
  e2eDaemonEnvironment: async ({ ompPeer, ompAgentDirectory }, use) => {
    await use({ AIT_SERVER_OMP_BIN: ompPeer, PI_CODING_AGENT_DIR: ompAgentDirectory });
  },
});
