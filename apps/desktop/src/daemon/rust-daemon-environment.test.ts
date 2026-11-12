import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { RustDaemonManager } from "./rust-daemon";

vi.mock("node:child_process", async (importOriginal) => {
  const original = await importOriginal<typeof import("node:child_process")>();
  return { ...original, spawn: vi.fn(original.spawn) };
});

afterEach(() => {
  vi.unstubAllEnvs();
  vi.clearAllMocks();
});

// The provider launch settings consumed by provider composition and the daemon CLI.
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

// Address, storage, authentication and unrelated settings stay owned by Desktop.
const FORBIDDEN_SERVER_ENVIRONMENT: Record<string, string> = {
  AIT_SERVER_LISTEN: "127.0.0.1:9999",
  AIT_SERVER_DATA_DIR: "/unrelated-runtime",
  AIT_SERVER_LOG_LEVEL: "debug",
  AIT_SERVER_MCP_SOCKET: "/tmp/unrelated-mcp.sock",
  AIT_SERVER_BIN: "/unrelated/daemon",
  AIT_SERVER_CODEX_BIN_EXTRA: "/native/codex-extra",
  AIT_SERVER_OMP_PROFILE: "unrelated-profile",
  AIT_SERVER_DEEPSEEK_HARNESS_BIN_BACKUP: "/native/dsh-backup",
  AIT_SERVER_CUSTOM_SECRET: "unrelated-secret",
};

it("passes every allowed provider setting through to the daemon unchanged", async () => {
  for (const [key, value] of Object.entries(ALLOWED_PROVIDER_ENVIRONMENT)) vi.stubEnv(key, value);
  for (const [key, value] of Object.entries(FORBIDDEN_SERVER_ENVIRONMENT)) vi.stubEnv(key, value);
  vi.stubEnv("AIT_SERVER_TOKEN", "explicit-runtime-token-at-least-32-characters");
  vi.stubEnv("AIT_APNS_KEY_FILE", "/private/test-apns.p8");
  const home = mkdtempSync(path.join(tmpdir(), "ait-provider-environment-"));
  const manager = new RustDaemonManager({
    binary: path.join(home, "missing-daemon"),
    home,
    listen: "127.0.0.1:54321",
    timeoutMs: 1000,
  });
  try {
    // Exercise the production spawn boundary without starting a real user daemon.
    await expect(manager.start()).rejects.toThrow();
    expect(spawn).toHaveBeenCalledOnce();
    const env = vi.mocked(spawn).mock.calls.at(-1)?.[2]?.env;
    for (const [key, value] of Object.entries(ALLOWED_PROVIDER_ENVIRONMENT)) {
      expect(env?.[key]).toBe(value);
    }
    for (const key of Object.keys(FORBIDDEN_SERVER_ENVIRONMENT)) {
      expect(env?.[key]).toBeUndefined();
    }
    // Desktop keeps owning address, storage and authentication.
    expect(env?.AIT_SERVER_TOKEN).toBe("explicit-runtime-token-at-least-32-characters");
    expect(env?.AIT_SERVER_HUB_NOTIFICATIONS).toBe("true");
    expect(env?.AIT_APNS_KEY_FILE).toBe("/private/test-apns.p8");
  } finally {
    await manager.stop();
    rmSync(home, { recursive: true, force: true });
  }
});

it("keeps the desktop isolation defaults when no provider settings are configured", async () => {
  for (const key of Object.keys(ALLOWED_PROVIDER_ENVIRONMENT)) vi.stubEnv(key, undefined);
  vi.stubEnv("AIT_SERVER_LISTEN", "127.0.0.1:9999");
  const home = mkdtempSync(path.join(tmpdir(), "ait-provider-environment-"));
  const manager = new RustDaemonManager({
    binary: path.join(home, "missing-daemon"),
    home,
    listen: "127.0.0.1:54321",
    timeoutMs: 1000,
  });
  try {
    await expect(manager.start()).rejects.toThrow();
    expect(spawn).toHaveBeenCalledOnce();
    const env = vi.mocked(spawn).mock.calls.at(-1)?.[2]?.env;
    for (const key of Object.keys(ALLOWED_PROVIDER_ENVIRONMENT)) {
      expect(env?.[key]).toBeUndefined();
    }
    expect(env?.AIT_SERVER_LISTEN).toBeUndefined();
    expect(env?.AIT_SERVER_HUB_NOTIFICATIONS).toBe("true");
  } finally {
    await manager.stop();
    rmSync(home, { recursive: true, force: true });
  }
});

it.each([
  { configured: undefined, expected: "true" },
  { configured: "true", expected: "true" },
  { configured: "false", expected: "false" },
  { configured: "0", expected: "0" },
])(
  "passes Hub notifications=$expected to the daemon when configured=$configured",
  async ({ configured, expected }) => {
    vi.stubEnv("AIT_SERVER_HUB_NOTIFICATIONS", configured);
    vi.stubEnv("AIT_SERVER_DATA_DIR", "/unrelated-runtime");
    vi.stubEnv("AIT_SERVER_TOKEN", "explicit-runtime-token-at-least-32-characters");
    vi.stubEnv("AIT_APNS_KEY_FILE", "/private/test-apns.p8");
    const home = mkdtempSync(path.join(tmpdir(), "ait-push-environment-"));
    const manager = new RustDaemonManager({
      binary: path.join(home, "missing-daemon"),
      home,
      listen: "127.0.0.1:54321",
      timeoutMs: 1000,
    });
    try {
      // Exercise the production spawn boundary without starting a real user daemon.
      await expect(manager.start()).rejects.toThrow();
      expect(spawn).toHaveBeenCalledOnce();
      const options = vi.mocked(spawn).mock.calls.at(-1)?.[2];
      expect(options?.env?.AIT_SERVER_HUB_NOTIFICATIONS).toBe(expected);
      expect(options?.env?.AIT_SERVER_DATA_DIR).toBeUndefined();
      expect(options?.env?.AIT_SERVER_TOKEN).toBe("explicit-runtime-token-at-least-32-characters");
      expect(options?.env?.AIT_APNS_KEY_FILE).toBe("/private/test-apns.p8");
    } finally {
      await manager.stop();
      rmSync(home, { recursive: true, force: true });
    }
  },
);
