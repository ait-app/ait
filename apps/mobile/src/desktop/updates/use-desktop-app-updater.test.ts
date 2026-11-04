/** @vitest-environment jsdom */
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildFakeCheckResult,
  buildFakeInstallResult,
} from "./test-utils/fake-desktop-app-updater-port";

const mocks = vi.hoisted(() => ({
  settings: { isLoading: true, releaseChannel: "stable" as "stable" | "beta" },
  checkDesktopAppUpdate: vi.fn(),
  installDesktopAppUpdate: vi.fn(),
}));

vi.mock("@/desktop/updates/desktop-updates", () => ({
  checkDesktopAppUpdate: mocks.checkDesktopAppUpdate,
  installDesktopAppUpdate: mocks.installDesktopAppUpdate,
  shouldShowDesktopUpdateSection: () => true,
  formatVersionWithPrefix: (version: string | null | undefined) => version ?? "",
}));
vi.mock("@/desktop/settings/desktop-settings", () => ({
  useDesktopSettings: () => ({
    settings: { releaseChannel: mocks.settings.releaseChannel },
    isLoading: mocks.settings.isLoading,
  }),
}));
vi.mock("@/desktop/hooks/desktop-ipc-error", () => ({
  useDesktopIpcErrorReporter: () => vi.fn(),
}));

async function loadHook() {
  vi.resetModules();
  return (await import("./use-desktop-app-updater")).useDesktopAppUpdater;
}

describe("useDesktopAppUpdater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.settings.isLoading = true;
    mocks.settings.releaseChannel = "stable";
    mocks.checkDesktopAppUpdate.mockResolvedValue(buildFakeCheckResult());
    mocks.installDesktopAppUpdate.mockResolvedValue(buildFakeInstallResult({ installed: true }));
  });

  it("waits for desktop settings before the automatic check", async () => {
    const useDesktopAppUpdater = await loadHook();
    const { rerender } = renderHook(() => useDesktopAppUpdater());
    expect(mocks.checkDesktopAppUpdate).not.toHaveBeenCalled();

    mocks.settings.isLoading = false;
    mocks.settings.releaseChannel = "beta";
    rerender();

    await waitFor(() => expect(mocks.checkDesktopAppUpdate).toHaveBeenCalledOnce());
    expect(mocks.checkDesktopAppUpdate).toHaveBeenCalledWith({
      releaseChannel: "beta",
      intent: "automatic",
    });
  });

  it("shares one updater between callers so a second install is ignored", async () => {
    mocks.settings.isLoading = false;
    let finishInstall: (value: unknown) => void = () => {};
    mocks.installDesktopAppUpdate.mockReturnValue(
      new Promise((resolve) => {
        finishInstall = resolve;
      }),
    );
    const useDesktopAppUpdater = await loadHook();
    const sidebar = renderHook(() => useDesktopAppUpdater());
    const settings = renderHook(() => useDesktopAppUpdater());

    let first: Promise<unknown> = Promise.resolve();
    act(() => {
      first = sidebar.result.current.installUpdate();
    });
    expect(settings.result.current.isInstalling).toBe(true);
    await act(async () => {
      expect(await settings.result.current.installUpdate()).toBeNull();
    });
    expect(mocks.installDesktopAppUpdate).toHaveBeenCalledOnce();

    await act(async () => {
      finishInstall(buildFakeInstallResult({ installed: true }));
      await first;
    });
    expect(settings.result.current.status).toBe("installed");
  });
});
