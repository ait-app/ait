import { afterEach, describe, expect, it, vi } from "vitest";
import { connectBrowserRelay, downloadBrowserAccountFile } from "./browser-account-relay";
import { nativeRelayHarness } from "./native-relay-test-harness";
import type * as NativeDownload from "./native-account-download";

const stream = vi.hoisted(() => vi.fn());
vi.mock("./native-account-download", () => ({ streamNativeAccountDownload: stream }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
  stream.mockReset();
});

async function nativeDownloadFixture() {
  const h = nativeRelayHarness();
  const actual = await vi.importActual<typeof NativeDownload>("./native-account-download");
  stream.mockImplementation((input) => actual.streamNativeAccountDownload(input, h.deps));
  return h;
}

describe("browser relay authentication", () => {
  it("passes the one-use ticket using the standard two-argument WebSocket constructor", () => {
    const opened = vi.fn();
    class BrowserSocket extends EventTarget {
      binaryType = "blob";
      constructor(...args: unknown[]) {
        super();
        opened(...args);
      }
      close() {}
    }
    vi.stubGlobal("WebSocket", BrowserSocket);
    connectBrowserRelay({
      url: "wss://console.test/api/v1/relay",
      headers: { Authorization: "Bearer one_use-ticket", "X-Unused": "value" },
    });
    expect(opened).toHaveBeenCalledExactlyOnceWith("wss://console.test/api/v1/relay", [
      "ait.ticket.one_use-ticket",
    ]);
  });
  it("rejects missing or malformed tickets before opening a socket", () => {
    const opened = vi.fn();
    vi.stubGlobal("WebSocket", opened);
    for (const authorization of [undefined, "Bearer ", "Bearer ticket,second", "Bearer a\\nb"]) {
      expect(() =>
        connectBrowserRelay({
          url: "wss://console.test/api/v1/relay",
          headers: authorization ? { Authorization: authorization } : undefined,
        }),
      ).toThrow("Invalid relay ticket");
    }
    expect(opened).not.toHaveBeenCalled();
  });
});

describe("browser artifact handoff", () => {
  it.each([3, null])(
    "rejects a short receipt-bound stream with length %s before creating a Blob",
    async (length) => {
      const h = await nativeDownloadFixture();
      const createObjectURL = vi.spyOn(URL, "createObjectURL");
      const pending = downloadBrowserAccountFile({
        hostId: "host",
        center: "https://example.test/api",
        token: "once",
        fileName: "session.md",
        mimeType: "text/markdown",
        expectedSize: 4,
        progress: vi.fn(),
      });
      const failed = expect(pending).rejects.toThrow(/mismatch|incomplete/);
      await h.flush();
      h.message({ type: "relay.ready", relay_session_id: "visit" });
      h.message({ type: "download.headers", status: 200, content_length: length });
      h.message(new Uint8Array([1, 2, 3]), true);
      h.message({ type: "download.end", bytes: 3 });
      await failed;
      expect(createObjectURL).not.toHaveBeenCalled();
      expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
    },
  );

  it("hands off only complete bytes while preserving each frame view's offset", async () => {
    vi.useFakeTimers();
    const h = await nativeDownloadFixture();
    const link = { href: "", download: "", click: vi.fn(), remove: vi.fn() };
    vi.stubGlobal("document", { createElement: () => link, body: { append: vi.fn() } });
    let blob: Blob | undefined;
    vi.spyOn(URL, "createObjectURL").mockImplementation((value) => {
      blob = value as Blob;
      return "blob:relay";
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const pending = downloadBrowserAccountFile({
      hostId: "host",
      center: "https://example.test/api",
      token: "once",
      fileName: "session.md",
      mimeType: "text/markdown",
      expectedSize: 3,
      progress: vi.fn(),
    });
    await h.flush();
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    h.message({ type: "download.headers", status: 200, content_length: null });
    h.message(new Uint8Array([9, 1, 2, 3, 9]).subarray(1, 4), true);
    expect(link.click).not.toHaveBeenCalled();
    h.message({ type: "download.end", bytes: 3 });
    await pending;
    expect(link.click).toHaveBeenCalledOnce();
    expect(new Uint8Array(await blob!.arrayBuffer())).toEqual(new Uint8Array([1, 2, 3]));
    await vi.advanceTimersByTimeAsync(60_000);
  });
});
