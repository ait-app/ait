import { describe, expect, it, vi } from "vitest";
import { streamNativeAccountDownload } from "./native-account-download";
import { nativeRelayHarness } from "./native-relay-test-harness";

describe("native relay downloads", () => {
  it("streams binary to disk in order and acknowledges the exact byte count", async () => {
    const h = nativeRelayHarness();
    const write = vi.fn();
    const progress = vi.fn();
    const finished = streamNativeAccountDownload(
      { hostId: "host", token: "download-once", write, progress },
      h.deps,
    );
    await h.flush();
    expect(h.account.openDownload).toHaveBeenCalledWith("host", "download-once");
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    h.message({ type: "download.headers", status: 200, content_length: 3 });
    const chunk = new Uint8Array([1, 2, 3]);
    h.message(chunk, true);
    h.message({ type: "download.end", bytes: 3 });
    await finished;
    expect(write).toHaveBeenCalledExactlyOnceWith(chunk);
    expect(h.socket.send).toHaveBeenCalledWith('{"type":"download.complete"}');
    expect(progress).toHaveBeenLastCalledWith(3, 3);
    expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
  });

  it.each(["wrong-pairing", "truncated", "cancelled", "disconnected"])(
    "rejects %s downloads and frees the relay",
    async (mode) => {
      const h = nativeRelayHarness();
      const finished = streamNativeAccountDownload(
        { hostId: "host", token: "once", write: vi.fn(), progress: vi.fn() },
        h.deps,
      );
      const failed = expect(finished).rejects.toBeInstanceOf(Error);
      await h.flush();
      h.message({
        type: "relay.ready",
        relay_session_id: mode === "wrong-pairing" ? "wrong" : "visit",
      });
      if (mode === "truncated") {
        h.message({ type: "download.headers", status: 200, content_length: 5 });
        h.message({ type: "download.end", bytes: 0 });
      }
      if (mode === "cancelled") h.cancel();
      if (mode === "disconnected") h.disconnect();
      await failed;
      expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
      expect(h.socket.send).not.toHaveBeenCalled();
    },
  );
});
