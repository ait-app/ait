import { afterEach, describe, expect, it, vi } from "vitest";
import type { HubSession } from "@ait/client/internal/hub";
import { CENTER, HOST, hubFixture } from "../../../../../packages/client/test/hub";
import { streamNativeAccountDownload } from "./native-account-download";
import { nativeRelayHarness } from "./native-relay-test-harness";

describe("native relay downloads", () => {
  it("streams binary to disk in order and acknowledges the exact byte count", async () => {
    const h = nativeRelayHarness();
    const write = vi.fn();
    const progress = vi.fn();
    const finished = streamNativeAccountDownload(
      {
        hostId: "host",
        center: "https://example.test/api",
        token: "download-once",
        write,
        progress,
      },
      h.deps,
    );
    await h.flush();
    expect(h.account.openDownload).toHaveBeenCalledWith(
      "host",
      "download-once",
      "https://example.test/api",
    );
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
        {
          hostId: "host",
          center: "https://example.test/api",
          token: "once",
          write: vi.fn(),
          progress: vi.fn(),
        },
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

describe("native Hub downloads", () => {
  async function hubDownload() {
    const h = nativeRelayHarness();
    const hub = hubFixture({
      ticket: async () => ({
        centerUrl: CENTER,
        ticket: "a".repeat(64),
        features: ["download", "credit"],
      }),
    });
    hub.link.start();
    await hub.flush();
    const socket = hub.socket();
    socket.open();
    socket.welcome({ f: ["credit", "download"] });
    const account = {
      ...h.account,
      // The fixture link comes from the SDK sources; the app types use its build.
      openHubDownload: vi.fn(
        async (_host: string, token: string) =>
          hub.link.openDownload(HOST.node_id, token) as unknown as HubSession,
      ),
    };
    const write = vi.fn();
    const finished = streamNativeAccountDownload(
      { hostId: "host", center: CENTER, token: "once", write, progress: vi.fn() },
      { ...h.deps, account: async () => account },
    );
    await vi.advanceTimersByTimeAsync(0);
    const id = socket.frames("open")[0]!.s as string;
    return { h, hub, socket, account, write, finished, id };
  }

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("streams over a Hub session, grants credit after writes and waits for done", async () => {
    const { h, socket, account, write, finished, id } = await hubDownload();
    expect(account.openHubDownload).toHaveBeenCalledWith("host", "once", CENTER);
    expect(socket.frames("open")[0]).toMatchObject({ k: "download", w: 1024 * 1024 });
    socket.receive({ t: "accept", seq: 1, s: id, w: 1024 * 1024 });
    socket.receive(
      { t: "msg", seq: 2, s: id, pt: "text" },
      JSON.stringify({ type: "download.headers", status: 200, content_length: 600_000 }),
    );
    socket.receive({ t: "msg", seq: 3, s: id, pt: "binary" }, new Uint8Array(600_000));
    expect(write).toHaveBeenCalledOnce();
    expect(socket.frames("credit")).toEqual([{ t: "credit", s: id, c: 600_000 }]);
    socket.receive(
      { t: "msg", seq: 4, s: id, pt: "text" },
      JSON.stringify({ type: "download.end", bytes: 600_000 }),
    );
    expect(new TextDecoder().decode(socket.sent.at(-1)!.payload)).toBe(
      '{"type":"download.complete"}',
    );
    let done = false;
    void finished.then(() => (done = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    socket.receive({ t: "close", seq: 5, s: id, code: "done" });
    await finished;
    expect(h.account.openDownload).not.toHaveBeenCalled();
    expect(socket.frames("close")).toEqual([]);
  });

  it("cancels the Hub session when the download fails", async () => {
    const { h, socket, finished, id } = await hubDownload();
    const failed = expect(finished).rejects.toThrow("cancelled");
    socket.receive({ t: "accept", seq: 1, s: id, w: 1024 * 1024 });
    h.cancel();
    await failed;
    expect(socket.frames("close")).toEqual([{ t: "close", seq: 2, s: id, code: "cancelled" }]);
  });

  it("falls back to the relay route when the Host answers unsupported", async () => {
    const { h, socket, finished, id } = await hubDownload();
    socket.receive({ t: "close", seq: 1, s: id, code: "unsupported" });
    await vi.advanceTimersByTimeAsync(0);
    expect(h.account.openDownload).toHaveBeenCalledWith("host", "once", CENTER);
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    h.message({ type: "download.headers", status: 200, content_length: 0 });
    h.message({ type: "download.end", bytes: 0 });
    await finished;
    expect(h.socket.send).toHaveBeenCalledWith('{"type":"download.complete"}');
  });
});
