import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile } from "@/types/host-connection";
import { useDownloadStore } from "./download-store";
import type { AgentTimelineExportResult } from "@ait/protocol/messages";

const mocks = vi.hoisted(() => ({
  files: new Set<string>(),
  close: vi.fn(),
  write: vi.fn(),
  share: vi.fn(),
  stream: vi.fn(),
  directDownload: vi.fn(),
  fetch: vi.fn(),
  failOpen: false,
  platform: "android",
}));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return mocks.platform;
    },
  },
}));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => undefined }));
vi.mock("@/constants/platform", () => ({ isWeb: false }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("@/runtime/browser-account", () => ({ supportsBrowserAccount: () => false }));
vi.mock("@/runtime/daemon-transport/browser-account-relay", () => ({
  downloadBrowserAccountFile: async () => {
    throw new Error("Unexpected browser transfer on a native platform");
  },
}));
vi.mock("@/runtime/daemon-transport/direct-download", () => ({ downloadFetch: mocks.fetch }));
vi.mock("expo-file-system/legacy", () => ({
  createDownloadResumable: (url: string, uri: string) => ({
    downloadAsync: () => mocks.directDownload(url, uri),
  }),
}));
vi.mock("expo-sharing", () => ({ isAvailableAsync: async () => true, shareAsync: mocks.share }));
vi.mock("@/utils/daemon-endpoints", () => ({
  buildDaemonWebSocketUrl: () => "ws://remote.test:6767/ws",
}));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));
vi.mock("@/runtime/daemon-transport/native-account-download", () => ({
  streamNativeAccountDownload: mocks.stream,
}));
vi.mock("expo-file-system", () => ({
  Paths: { cache: "file:///cache" },
  File: class {
    uri: string;
    constructor(...parts: string[]) {
      this.uri = parts.join("/");
    }
    get exists() {
      return mocks.files.has(this.uri);
    }
    create() {
      mocks.files.add(this.uri);
    }
    open() {
      if (mocks.failOpen) throw new Error("Cannot open file");
      return { writeBytes: mocks.write, close: mocks.close };
    }
    delete() {
      mocks.files.delete(this.uri);
    }
    move(target: { uri: string }) {
      mocks.files.delete(this.uri);
      this.uri = target.uri;
      mocks.files.add(this.uri);
    }
  },
}));

beforeEach(() => {
  vi.resetAllMocks();
  mocks.files.clear();
  mocks.failOpen = false;
  mocks.platform = "android";
  useDownloadStore.setState({ downloads: new Map(), activeDownloadId: null });
});

const relayConnection = {
  id: "relay",
  type: "accountRelay",
  hostId: "host",
  center: "https://center.test/api",
} as const;
const directConnection = { id: "direct", type: "directTcp", endpoint: "remote.test:6767" } as const;

async function download(connection: HostProfile["connections"][number] = relayConnection) {
  await useDownloadStore.getState().startDownload({
    serverId: "server",
    scopeId: "workspace",
    fileName: "report.txt",
    path: "report.txt",
    daemonProfile: { connections: [connection] } as HostProfile,
    requestFileDownloadToken: async () => ({
      token: "once",
      fileName: "report.txt",
      mimeType: "text/plain",
      error: null,
    }),
  });
  return [...useDownloadStore.getState().downloads.values()][0]!;
}

describe("native mobile relay download files", () => {
  it.each(["android", "ios"])(
    "writes %s chunks and only shares the complete download",
    async (platform) => {
      mocks.platform = platform;
      mocks.stream.mockImplementation(async ({ write, progress }) => {
        write(new Uint8Array([1, 2, 3]));
        progress(3, 3);
      });
      expect(await download()).toMatchObject({
        status: "complete",
        progress: { bytesWritten: 3, percent: 1 },
      });
      expect(mocks.close).toHaveBeenCalledOnce();
      expect(mocks.files).toEqual(new Set(["file:///cache/report.txt"]));
      expect(mocks.share).toHaveBeenCalledWith(
        "file:///cache/report.txt",
        expect.objectContaining({ mimeType: "text/plain" }),
      );
    },
  );

  it.each(["interrupted", "open-failure"])("removes partial files after %s", async (failure) => {
    mocks.platform = "ios";
    mocks.failOpen = failure === "open-failure";
    mocks.stream.mockRejectedValue(new Error("Download interrupted"));
    expect(await download()).toMatchObject({ status: "error" });
    expect(mocks.files.size).toBe(0);
    expect(mocks.share).not.toHaveBeenCalled();
    expect(mocks.close).toHaveBeenCalledTimes(failure === "interrupted" ? 1 : 0);
  });

  it("gives concurrent downloads of the same name distinct files", async () => {
    mocks.platform = "ios";
    const pending: Array<() => void> = [];
    mocks.stream.mockImplementation(
      () => new Promise<void>((resolve) => pending.push(() => resolve())),
    );
    const first = download();
    const second = download();
    await vi.waitFor(() => expect(pending).toHaveLength(2));
    for (const finish of pending) finish();
    await Promise.all([first, second]);
    expect(mocks.files).toEqual(
      new Set(["file:///cache/report.txt", "file:///cache/report (1).txt"]),
    );
  });
});

describe("native mobile direct-host download files", () => {
  it("shares a successful download", async () => {
    mocks.directDownload.mockImplementation(async (_url: string, uri: string) => {
      mocks.files.add(uri);
      return { uri, status: 200, headers: {} };
    });
    expect(await download(directConnection)).toMatchObject({ status: "complete" });
    expect(mocks.share).toHaveBeenCalledWith("file:///cache/report.txt", expect.anything());
  });

  it("removes an HTTP error body instead of sharing it as the file", async () => {
    mocks.directDownload.mockImplementation(async (_url: string, uri: string) => {
      mocks.files.add(uri);
      return { uri, status: 404, headers: {} };
    });
    expect(await download(directConnection)).toMatchObject({
      status: "error",
      message: "downloads.failed (HTTP 404)",
    });
    expect(mocks.files.size).toBe(0);
    expect(mocks.share).not.toHaveBeenCalled();
  });
});

describe("native timeline export consumer", () => {
  const artifact: AgentTimelineExportResult = {
    agentId: "agent",
    format: "html",
    scope: "timeline",
    source: "persisted_timeline",
    token: "once",
    fileName: "timeline.html",
    mimeType: "text/html",
    size: 3,
    entries: 1,
    expiresAt: "2026-11-11T01:00:00Z",
  };
  it.each(["android", "ios"])(
    "streams %s export bytes into a final file before sharing",
    async (platform) => {
      mocks.platform = platform;
      mocks.stream.mockImplementation(async ({ write }) => write(new Uint8Array([1, 2, 3])));
      const release = vi.fn();
      await useDownloadStore.getState().startTimelineExport({
        serverId: "host",
        agentId: "agent",
        daemonProfile: { connections: [relayConnection] } as HostProfile,
        generate: async () => artifact,
        release,
      });
      expect(mocks.write).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]));
      expect(mocks.files).toEqual(new Set(["file:///cache/timeline.html"]));
      expect(mocks.share).toHaveBeenCalledWith(
        "file:///cache/timeline.html",
        expect.objectContaining({ mimeType: "text/html" }),
      );
      expect(mocks.stream).toHaveBeenCalledWith(
        expect.objectContaining({
          maxBytes: 16 * 1024 * 1024,
          expectedSize: artifact.size,
          signal: expect.any(AbortSignal),
        }),
      );
      expect(release).not.toHaveBeenCalled();
      expect([...useDownloadStore.getState().downloads.values()][0]!.status).toBe("complete");
    },
  );

  it("aborts a partial stream, closes and deletes it without sharing or regenerating", async () => {
    mocks.stream.mockImplementation(({ signal, write }) => {
      write(new Uint8Array([1]));
      const { promise, reject } = Promise.withResolvers<void>();
      signal.addEventListener("abort", () => reject(new Error("Cancelled")), { once: true });
      return promise;
    });
    const generate = vi.fn(async () => artifact);
    const release = vi.fn(async () => ({ released: true }));
    const pending = useDownloadStore.getState().startTimelineExport({
      serverId: "host",
      agentId: "agent",
      daemonProfile: { connections: [relayConnection] } as HostProfile,
      generate,
      release,
    });
    await vi.waitFor(() => expect(mocks.write).toHaveBeenCalledOnce());
    [...useDownloadStore.getState().downloads.values()][0]!.cancel!();
    await pending;
    expect(mocks.close).toHaveBeenCalledOnce();
    expect(mocks.files.size).toBe(0);
    expect(mocks.share).not.toHaveBeenCalled();
    expect(generate).toHaveBeenCalledOnce();
    expect(release).toHaveBeenCalledExactlyOnceWith("once");
  });

  it.each(["stream-failure", "destination-conflict"])(
    "preserves a foreign file occupying the reserved target on %s",
    async (mode) => {
      mocks.stream.mockImplementation(async ({ write }) => {
        write(new Uint8Array([1, 2, 3]));
        mocks.files.add("file:///cache/timeline.html");
        if (mode === "stream-failure") throw new Error("Transfer failed");
      });
      const release = vi.fn(async () => ({ released: true }));
      await useDownloadStore.getState().startTimelineExport({
        serverId: "host",
        agentId: "agent",
        daemonProfile: { connections: [relayConnection] } as HostProfile,
        generate: async () => artifact,
        release,
      });
      expect(mocks.files).toEqual(new Set(["file:///cache/timeline.html"]));
      expect(mocks.share).not.toHaveBeenCalled();
      expect([...useDownloadStore.getState().downloads.values()][0]!.status).toBe("error");
      expect(release).toHaveBeenCalledExactlyOnceWith("once");
    },
  );

  it("removes its moved file when sharing is cancelled without reporting completion", async () => {
    mocks.stream.mockImplementation(async ({ write }) => write(new Uint8Array([1, 2, 3])));
    mocks.share.mockRejectedValue(new Error("Share cancelled"));
    const release = vi.fn(async () => ({ released: true }));
    await useDownloadStore.getState().startTimelineExport({
      serverId: "host",
      agentId: "agent",
      daemonProfile: { connections: [relayConnection] } as HostProfile,
      generate: async () => artifact,
      release,
    });
    expect(mocks.files.size).toBe(0);
    expect([...useDownloadStore.getState().downloads.values()][0]!.status).toBe("error");
    expect(release).toHaveBeenCalledExactlyOnceWith("once");
  });

  it.each(["android", "ios"])(
    "streams direct HTTP into %s files without a buffered fetch fallback",
    async (platform) => {
      mocks.platform = platform;
      mocks.fetch.mockResolvedValue(new Response(new Uint8Array([1, 2, 3])));
      const release = vi.fn();
      await useDownloadStore.getState().startTimelineExport({
        serverId: "host",
        agentId: "agent",
        daemonProfile: { connections: [directConnection] } as HostProfile,
        generate: async () => artifact,
        release,
      });
      expect(mocks.write).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]));
      expect(mocks.files).toEqual(new Set(["file:///cache/timeline.html"]));
      expect(mocks.fetch).toHaveBeenCalledWith(
        "http://remote.test:6767/api/files/download?token=once",
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      );
      expect(release).not.toHaveBeenCalled();
    },
  );
});
