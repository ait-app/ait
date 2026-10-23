import { afterEach, describe, expect, it, vi } from "vitest";
import { isElectronRuntime } from "@/desktop/host";
import { shouldUseDesktopDaemon } from "./desktop-daemon";

describe("built-in desktop daemon availability", () => {
  afterEach(() => vi.unstubAllGlobals());

  it.each(["darwin", "linux", "win32"])("selects daemon support on %s", (platform) => {
    vi.stubGlobal("window", { paseoDesktop: { platform } });
    expect(isElectronRuntime()).toBe(true);
    expect(shouldUseDesktopDaemon()).toBe(platform !== "win32");
  });

  it("does not expose a built-in daemon in a browser", () => {
    vi.stubGlobal("window", {});
    expect(shouldUseDesktopDaemon()).toBe(false);
  });
});
