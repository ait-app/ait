/** @vitest-environment jsdom */
import React, { type ReactNode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopDaemonStatus } from "@/desktop/daemon/desktop-daemon";

const mocks = vi.hoisted(() => ({
  reportError: vi.fn(),
  startDesktopDaemon: vi.fn(),
  stopDesktopDaemon: vi.fn(),
  upsertDesktopDaemonConnection: vi.fn(),
}));

vi.mock("@/desktop/hooks/desktop-ipc-error", () => ({
  useDesktopIpcErrorReporter: () => mocks.reportError,
}));
vi.mock("@/desktop/daemon/desktop-daemon", () => ({
  startDesktopDaemon: mocks.startDesktopDaemon,
  stopDesktopDaemon: mocks.stopDesktopDaemon,
}));
vi.mock("@/runtime/host-runtime", () => ({ getHostRuntimeStore: () => ({}) }));
vi.mock("@/runtime/daemon-start-service", () => ({
  upsertDesktopDaemonConnection: mocks.upsertDesktopDaemonConnection,
}));
vi.mock("@/utils/confirm-dialog", () => ({ confirmDialog: vi.fn(async () => true) }));

import { useBuiltInDaemonManagement } from "./use-built-in-daemon-management";

const RUNNING_STATUS = { status: "running", ownedByDesktop: true } as DesktopDaemonStatus;

function renderManagement(updateSettings: () => Promise<unknown>) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return renderHook(
    () =>
      useBuiltInDaemonManagement({
        daemonStatus: null,
        settings: {
          manageBuiltInDaemon: false,
          keepRunningAfterQuit: false,
          listen: "127.0.0.1:0",
        },
        updateSettings,
        setStatus: vi.fn(),
        refreshStatus: vi.fn(),
      }),
    { wrapper },
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  mocks.startDesktopDaemon.mockResolvedValue(RUNNING_STATUS);
  mocks.upsertDesktopDaemonConnection.mockResolvedValue({ ok: true });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("useBuiltInDaemonManagement", () => {
  it("leaves a failed settings save to the settings mutation, which already reported it", async () => {
    const { result } = renderManagement(async () => {
      throw new Error("disk unavailable");
    });

    await act(async () => {
      expect(await result.current.enable()).toBeNull();
    });

    expect(mocks.startDesktopDaemon).not.toHaveBeenCalled();
    expect(mocks.reportError).not.toHaveBeenCalled();
  });

  it("still reports a daemon failure that happens after the settings saved", async () => {
    mocks.startDesktopDaemon.mockRejectedValue(new Error("spawn failed"));
    const { result } = renderManagement(async () => undefined);

    await act(async () => {
      expect(await result.current.enable()).toBeNull();
    });

    expect(mocks.reportError).toHaveBeenCalledTimes(1);
    expect(mocks.reportError).toHaveBeenCalledWith(
      expect.objectContaining({
        logLabel: "[Settings] Failed to update built-in daemon management",
      }),
    );
  });
});
