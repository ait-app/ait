import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentTimelineExportResult } from "@ait/protocol/messages";
import type { HostProfile } from "@/types/host-connection";
import { useDownloadStore } from "./download-store";

const browser = vi.hoisted(() => ({ account: false, download: vi.fn() }));
const desktop = vi.hoisted(() => ({ enabled: false, invoke: vi.fn() }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => (desktop.enabled ? { invoke: desktop.invoke } : undefined),
}));
vi.mock("@/constants/platform", () => ({ isWeb: true }));
vi.mock("@/runtime/browser-account", () => ({ supportsBrowserAccount: () => browser.account }));
vi.mock("@/runtime/daemon-transport/browser-account-relay", () => ({
  downloadBrowserAccountFile: browser.download,
}));
vi.mock("expo-file-system", () => ({ File: class {}, Paths: {} }));
vi.mock("expo-file-system/legacy", () => ({}));
vi.mock("expo-sharing", () => ({}));
vi.mock("@/i18n/i18next", () => ({ i18n: { t: (key: string) => key } }));
vi.mock("@/utils/daemon-endpoints", () => ({ buildDaemonWebSocketUrl: () => "ws://host.test/ws" }));
vi.mock("@/utils/open-external-url", () => ({ openExternalUrl: vi.fn() }));

const artifact: AgentTimelineExportResult = {
  agentId: "agent",
  format: "markdown",
  scope: "conversation",
  source: "persisted_timeline",
  token: "single-use",
  fileName: "session.md",
  mimeType: "text/markdown",
  size: 5,
  entries: 1,
  expiresAt: "2026-11-11T01:00:00Z",
};
const generate = vi.fn(async () => artifact);
const release = vi.fn(async () => ({ released: true }));
const fetchFile = vi.fn<typeof fetch>();
const link = { href: "", download: "", rel: "", click: vi.fn(), remove: vi.fn() };
const blobs: Blob[] = [];

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  blobs.length = 0;
  browser.account = false;
  desktop.enabled = false;
  useDownloadStore.setState({ downloads: new Map(), activeDownloadId: null });
  vi.stubGlobal("fetch", fetchFile);
  vi.stubGlobal("document", { createElement: () => link, body: { appendChild: vi.fn() } });
  vi.spyOn(URL, "createObjectURL").mockImplementation((blob) => {
    blobs.push(blob as Blob);
    return "blob:export";
  });
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function start() {
  return useDownloadStore.getState().startTimelineExport({
    serverId: "host",
    agentId: "agent",
    generate,
    release,
    daemonProfile: {
      connections: [{ id: "direct", type: "directTcp", endpoint: "host.test", password: "secret" }],
    } as HostProfile,
  });
}
function current() {
  return [...useDownloadStore.getState().downloads.values()][0]!;
}

describe("timeline export browser transfer", () => {
  it("waits for EOF then hands exact bytes and filename to the browser without claiming a disk save", async () => {
    let body!: ReadableStreamDefaultController<Uint8Array>;
    fetchFile.mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            body = controller;
          },
        }),
      ),
    );
    const pending = start();
    await vi.advanceTimersByTimeAsync(0);
    body.enqueue(new TextEncoder().encode("hello"));
    await vi.advanceTimersByTimeAsync(0);
    expect(current().status).toBe("downloading");
    expect(link.click).not.toHaveBeenCalled();
    body.close();
    await pending;
    expect(current().status).toBe("browser_handoff");
    expect(await blobs[0]!.text()).toBe("hello");
    expect(link.download).toBe("session.md");
    expect(fetchFile).toHaveBeenCalledExactlyOnceWith(
      "http://host.test/api/files/download?token=single-use",
      expect.objectContaining({
        headers: { Authorization: "Bearer secret" },
        credentials: "omit",
        redirect: "error",
        signal: expect.any(AbortSignal),
      }),
    );
    expect(generate).toHaveBeenCalledOnce();
    expect(release).not.toHaveBeenCalled();
  });

  it.each([404, 410, 500])("does not regenerate after HTTP %s", async (status) => {
    fetchFile.mockResolvedValue(new Response("expired", { status }));
    await start();
    expect(current().status).toBe("error");
    expect(generate).toHaveBeenCalledOnce();
    expect(fetchFile).toHaveBeenCalledOnce();
    expect(release).toHaveBeenCalledExactlyOnceWith("single-use");
    expect(link.click).not.toHaveBeenCalled();
  });

  it("cancels the fetch and releases the unused artifact", async () => {
    fetchFile.mockImplementation((_url, init) => {
      const { promise, reject } = Promise.withResolvers<Response>();
      init!.signal!.addEventListener(
        "abort",
        () => reject(new DOMException("Cancelled", "AbortError")),
        { once: true },
      );
      return promise;
    });
    const pending = start();
    await vi.advanceTimersByTimeAsync(0);
    current().cancel!();
    await pending;
    expect(current()).toMatchObject({ status: "error", message: "downloads.cancelled" });
    expect(release).toHaveBeenCalledExactlyOnceWith("single-use");
    expect(generate).toHaveBeenCalledOnce();
    expect(link.click).not.toHaveBeenCalled();
  });

  it("releases a late generation result after cancellation without consuming it", async () => {
    const { promise, resolve } = Promise.withResolvers<AgentTimelineExportResult>();
    generate.mockReturnValueOnce(promise);
    const pending = start();
    current().cancel!();
    resolve(artifact);
    await pending;
    expect(fetchFile).not.toHaveBeenCalled();
    expect(release).toHaveBeenCalledExactlyOnceWith("single-use");
  });

  it("rejects oversized streaming bodies without creating a partial browser download", async () => {
    const cancel = vi.fn();
    fetchFile.mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(new Uint8Array(16 * 1024 * 1024));
            controller.enqueue(new Uint8Array(1));
          },
          cancel,
        }),
      ),
    );
    await start();
    expect(current()).toMatchObject({ status: "error", message: "timelineExport.tooLarge" });
    expect(cancel).toHaveBeenCalledOnce();
    expect(blobs).toHaveLength(0);
    expect(release).toHaveBeenCalledOnce();
  });

  it("keeps relay exports pending until transfer finishes, then reports browser handoff", async () => {
    browser.account = true;
    const transfer = Promise.withResolvers<void>();
    browser.download.mockReturnValueOnce(transfer.promise);
    const pending = useDownloadStore.getState().startTimelineExport({
      serverId: "host",
      agentId: "agent",
      generate,
      release,
      daemonProfile: {
        connections: [
          {
            id: "relay",
            type: "accountRelay",
            hostId: "remote-host",
            center: "https://account.test/api",
          },
        ],
      } as HostProfile,
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(current().status).toBe("downloading");
    transfer.resolve();
    await pending;
    expect(current().status).toBe("browser_handoff");
    expect(browser.download).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({
        hostId: "remote-host",
        center: "https://account.test/api",
        token: artifact.token,
        fileName: artifact.fileName,
        maxBytes: 16 * 1024 * 1024,
        expectedSize: artifact.size,
      }),
    );
    expect(fetchFile).not.toHaveBeenCalled();
    expect(release).not.toHaveBeenCalled();
  });

  it("passes the receipt size to desktop and reports a rejected transfer as failed, not complete", async () => {
    desktop.enabled = true;
    const transfer = Promise.withResolvers<void>();
    desktop.invoke.mockImplementation((method) => {
      if (method === "account_download_prepare") return Promise.resolve("preparation");
      if (method === "account_download") return transfer.promise;
      return Promise.resolve();
    });
    const pending = start();
    await vi.advanceTimersByTimeAsync(0);
    expect(current().status).toBe("downloading");
    expect(desktop.invoke).toHaveBeenCalledWith(
      "account_download_prepare",
      expect.objectContaining({
        expectedSize: artifact.size,
        maxBytes: 16 * 1024 * 1024,
        direct: expect.objectContaining({ baseUrl: expect.any(String) }),
      }),
    );
    transfer.reject(new Error("File size mismatch"));
    await pending;
    expect(current().status).toBe("error");
    expect(release).toHaveBeenCalledExactlyOnceWith("single-use");
    expect(generate).toHaveBeenCalledOnce();
    expect(fetchFile).not.toHaveBeenCalled();
  });
});
