import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getDesktopHost } from "@/desktop/host";
import { buildDaemonWebSocketUrl } from "@/utils/daemon-endpoints";
import type { HostProfile } from "@/types/host-connection";
import { useDownloadStore } from "./download-store";

vi.mock("@/desktop/host", () => ({ getDesktopHost: vi.fn() }));
vi.mock("expo-file-system", () => ({
  File: class {
    uri = "file:///report.txt";
    exists = false;
  },
  Paths: { cache: "file:///cache" },
}));
vi.mock("expo-file-system/legacy", () => ({
  createDownloadResumable: () => ({ downloadAsync: async () => ({ uri: "file:///report.txt" }) }),
}));
vi.mock("expo-sharing", () => ({ isAvailableAsync: async () => false }));
vi.mock("@/utils/daemon-endpoints", () => ({ buildDaemonWebSocketUrl: vi.fn() }));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));
vi.mock("@/constants/platform", () => ({ isWeb: false }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));

beforeEach(() => {
  vi.resetAllMocks();
  useDownloadStore.setState({ downloads: new Map(), activeDownloadId: null });
});
afterEach(() => vi.useRealTimers());

function fixture(activeConnectionId?: string) {
  const invoke = vi.fn(
    async (command: string, _args?: Record<string, unknown>): Promise<unknown> =>
      command === "account_download_prepare" ? "prepared-download" : undefined,
  );
  const remove = vi.fn();
  vi.mocked(getDesktopHost).mockReturnValue({ invoke, events: { on: vi.fn(() => remove) } });
  const requestFileDownloadToken = vi.fn(async () => ({
    token: `token-${Date.now()}`,
    fileName: "report.txt",
    mimeType: "text/plain",
    error: null as string | null,
  }));
  const input = {
    serverId: "server",
    scopeId: "/workspace",
    fileName: "report.txt",
    path: "report.txt",
    daemonProfile: {
      connections: [
        { id: "direct", type: "directTcp", endpoint: "remote.test:6767" },
        { id: "relay", type: "accountRelay", hostId: "host", center: "https://center.test/api" },
      ],
    } as HostProfile,
    activeConnectionId,
    requestFileDownloadToken,
  };
  return {
    invoke,
    remove,
    requestFileDownloadToken,
    start: () => useDownloadStore.getState().startDownload(input),
    result: () => [...useDownloadStore.getState().downloads.values()][0],
  };
}

describe("relay download preparation", () => {
  it("uses the active direct connection when the host also has a saved online-service connection", async () => {
    const { invoke, start, result } = fixture("direct");
    vi.mocked(buildDaemonWebSocketUrl).mockReturnValue("ws://remote.test:6767/v1/ws");
    await start();
    expect(invoke).not.toHaveBeenCalled();
    expect(buildDaemonWebSocketUrl).toHaveBeenCalledWith("remote.test:6767", { useTls: false });
    expect(result().status).toBe("complete");
  });
  it("requests a fresh token only after a save dialog lasting longer than the token lifetime", async () => {
    vi.useFakeTimers();
    const { invoke, requestFileDownloadToken, start, result, remove } = fixture();
    let confirm!: (id: string) => void;
    const selection = new Promise<string>((resolve) => {
      confirm = resolve;
    });
    invoke.mockImplementation(async (command) =>
      command === "account_download_prepare" ? selection : undefined,
    );
    const downloading = start();
    await vi.advanceTimersByTimeAsync(90_000);
    expect(requestFileDownloadToken).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("account_download_prepare", {
      hostId: "host",
      center: "https://center.test/api",
      fileName: "report.txt",
      downloadId: expect.any(String),
    });

    confirm("prepared-download");
    await downloading;
    expect(requestFileDownloadToken).toHaveBeenCalledExactlyOnceWith("report.txt");
    expect(invoke).toHaveBeenCalledWith("account_download", {
      preparationId: "prepared-download",
      token: `token-${Date.now()}`,
    });
    expect(result().status).toBe("complete");
    expect(remove).toHaveBeenCalledOnce();
  });

  it("does not request a token when the save dialog is cancelled", async () => {
    const { invoke, requestFileDownloadToken, start, result } = fixture();
    invoke.mockRejectedValueOnce(new Error("Download cancelled."));
    await start();
    expect(requestFileDownloadToken).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(result()).toMatchObject({ status: "error", message: "Download cancelled." });
  });

  it("releases the preparation when requesting the token fails", async () => {
    const { invoke, requestFileDownloadToken, start, result } = fixture();
    requestFileDownloadToken.mockRejectedValueOnce(new Error("Host disconnected."));
    await start();
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "account_download_prepare",
      "account_download_cancel",
    ]);
    expect(invoke).toHaveBeenLastCalledWith("account_download_cancel", {
      preparationId: "prepared-download",
    });
    expect(result()).toMatchObject({ status: "error", message: "Host disconnected." });
  });

  it("removes the progress listener and releases the preparation when transfer fails", async () => {
    const { invoke, start, result, remove } = fixture();
    invoke.mockImplementation(async (command) => {
      if (command === "account_download") throw new Error("Download interrupted.");
      return command === "account_download_prepare" ? "prepared-download" : undefined;
    });
    await start();
    expect(remove).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenLastCalledWith("account_download_cancel", {
      preparationId: "prepared-download",
    });
    expect(result()).toMatchObject({ status: "error", message: "Download interrupted." });
  });
});
