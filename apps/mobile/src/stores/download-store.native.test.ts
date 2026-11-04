import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile } from "@/types/host-connection";
import { useDownloadStore } from "./download-store";

const mocks = vi.hoisted(() => ({
  files: new Set<string>(),
  close: vi.fn(),
  write: vi.fn(),
  share: vi.fn(),
  stream: vi.fn(),
  directDownload: vi.fn(),
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
