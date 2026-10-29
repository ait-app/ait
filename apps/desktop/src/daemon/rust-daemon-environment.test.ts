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
