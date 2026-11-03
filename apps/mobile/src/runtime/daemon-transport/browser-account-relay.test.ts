import { afterEach, describe, expect, it, vi } from "vitest";
import { connectBrowserRelay } from "./browser-account-relay";

afterEach(() => vi.unstubAllGlobals());

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
