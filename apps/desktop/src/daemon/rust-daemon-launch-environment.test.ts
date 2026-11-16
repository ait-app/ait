import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { RustDaemonManager } from "./rust-daemon";

// The launcher must hand these to the daemon process itself, not merely to a mocked spawn.
const ALLOWED_PROVIDER_ENVIRONMENT: Record<string, string> = {
  AIT_SERVER_CODEX_BIN: "/native/codex",
  AIT_SERVER_CLAUDE_BIN: "/native/claude",
  AIT_SERVER_ANTIGRAVITY_BIN: "/native/antigravity",
  AIT_SERVER_OPENCODE_BIN: "/native/opencode",
  AIT_SERVER_DEEPSEEK_HARNESS_BIN: "/native/dsh",
  AIT_SERVER_DEEPSEEK_HARNESS_PROFILE: "desktop-profile",
  AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT: "acp",
  AIT_SERVER_PI_BIN: "/native/pi",
  AIT_SERVER_OMP_BIN: "/native/omp",
  AIT_SERVER_MINIMAX_CODE_BIN: "/native/mcode",
};

const FORBIDDEN_SERVER_ENVIRONMENT: Record<string, string> = {
  AIT_SERVER_LISTEN: "127.0.0.1:9999",
  AIT_SERVER_DATA_DIR: "/unrelated-runtime",
  AIT_SERVER_LOG_LEVEL: "debug",
  AIT_SERVER_MCP_SOCKET: "/tmp/unrelated-mcp.sock",
  AITD_BIN: "/unrelated/daemon",
  AIT_SERVER_BIN: "/legacy/daemon",
  AIT_SERVER_CUSTOM_SECRET: "unrelated-secret",
};

const cleanups: (() => Promise<void>)[] = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.unstubAllEnvs();
});

it.skipIf(process.platform === "win32")(
  "delivers the provider allowlist to a launched daemon and keeps unrelated settings out",
  async () => {
    const home = mkdtempSync(path.join(tmpdir(), "ait-launch-environment-"));
    const script = fileURLToPath(new URL("./fixtures/environment-daemon.cjs", import.meta.url));
    const binary = path.join(home, "daemon");
    writeFileSync(binary, `#!${process.execPath}\nrequire(${JSON.stringify(script)});\n`, {
      mode: 0o700,
    });
    for (const [key, value] of Object.entries(ALLOWED_PROVIDER_ENVIRONMENT)) vi.stubEnv(key, value);
    for (const [key, value] of Object.entries(FORBIDDEN_SERVER_ENVIRONMENT)) vi.stubEnv(key, value);
    vi.stubEnv("AIT_SERVER_TOKEN", "explicit-runtime-token-at-least-32-characters");
    const manager = new RustDaemonManager({
      binary,
      home,
      listen: "127.0.0.1:0",
      timeoutMs: 5000,
    });
    cleanups.push(async () => {
      await manager.stop();
      rmSync(home, { recursive: true, force: true });
    });

    const status = await manager.start();
    expect(status).toMatchObject({ status: "running", ownedByDesktop: true });

    // The child wrote the environment it actually inherited from the launcher.
    const received = JSON.parse(
      readFileSync(path.join(home, "received-env.json"), "utf8"),
    ) as Record<string, string>;
    for (const [key, value] of Object.entries(ALLOWED_PROVIDER_ENVIRONMENT)) {
      expect(received[key]).toBe(value);
    }
    for (const key of Object.keys(FORBIDDEN_SERVER_ENVIRONMENT)) {
      expect(received[key]).toBeUndefined();
    }
    expect(received.AIT_SERVER_TOKEN).toBe("explicit-runtime-token-at-least-32-characters");
    expect(received.AIT_SERVER_HUB_NOTIFICATIONS).toBe("true");

    const stopped = await manager.stop();
    expect(stopped).toMatchObject({ status: "stopped", ownedByDesktop: false });
  },
);
