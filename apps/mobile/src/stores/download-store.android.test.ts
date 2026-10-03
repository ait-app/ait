import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProfile } from "@/types/host-connection";
import { useDownloadStore } from "./download-store";

const mocks = vi.hoisted(() => ({
  files: new Set<string>(),
  close: vi.fn(),
  write: vi.fn(),
  share: vi.fn(),
  stream: vi.fn(),
  failOpen: false,
}));
vi.mock("react-native", () => ({ Platform: { OS: "android" } }));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => undefined }));
vi.mock("@/constants/platform", () => ({ isWeb: false }));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("expo-file-system/legacy", () => ({}));
vi.mock("expo-sharing", () => ({ isAvailableAsync: async () => true, shareAsync: mocks.share }));
vi.mock("@/utils/daemon-endpoints", () => ({ buildDaemonWebSocketUrl: vi.fn() }));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));
vi.mock("@/runtime/rust-daemon/native-account-download", () => ({
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
  useDownloadStore.setState({ downloads: new Map(), activeDownloadId: null });
});

async function download() {
  await useDownloadStore.getState().startDownload({
    serverId: "server",
    scopeId: "workspace",
    fileName: "report.txt",
    path: "report.txt",
    daemonProfile: {
      connections: [{ id: "relay", type: "accountRelay", hostId: "host" }],
    } as HostProfile,
    requestFileDownloadToken: async () => ({
      token: "once",
      fileName: "report.txt",
      mimeType: "text/plain",
      error: null,
    }),
  });
  return [...useDownloadStore.getState().downloads.values()][0]!;
}

describe("Android relay download files", () => {
  it("writes chunks, closes the file and only shares the complete download", async () => {
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
  });

  it.each(["interrupted", "open-failure"])("removes partial files after %s", async (failure) => {
    mocks.failOpen = failure === "open-failure";
    mocks.stream.mockRejectedValue(new Error("Download interrupted"));
    expect(await download()).toMatchObject({ status: "error" });
    expect(mocks.files.size).toBe(0);
    expect(mocks.share).not.toHaveBeenCalled();
    expect(mocks.close).toHaveBeenCalledTimes(failure === "interrupted" ? 1 : 0);
  });
});
