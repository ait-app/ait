import { afterEach, describe, expect, it, vi } from "vitest";
import { HOST, hubFixture } from "../../test/hub.js";
import { HubSessionClosedError } from "./session.js";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

async function session() {
  const h = hubFixture();
  const socket = await h.online();
  const transport = h.link.openSession(HOST.node_id);
  const events = { open: vi.fn(), close: vi.fn(), error: vi.fn(), message: vi.fn() };
  transport.onOpen(events.open);
  transport.onClose(events.close);
  transport.onError(events.error);
  transport.onMessage(events.message);
  return { h, socket, transport, events };
}

describe("hub session transport", () => {
  it("opens asynchronously and sends the business hello as the open payload", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    expect(events.open).toHaveBeenCalledOnce();
    const later = h.link.openSession(HOST.node_id);
    const open = vi.fn();
    later.onOpen(open);
    expect(open).not.toHaveBeenCalled();
    await h.flush();
    expect(open).toHaveBeenCalledOnce();
    expect(() => transport.send(new Uint8Array([1]))).toThrow("Expected the AIT hello");
    transport.send('{"type":"hello","client_id":"android"}');
    expect(socket.frames("open")).toEqual([
      { t: "open", seq: 1, s: transport.id, to: { node: HOST.node_id } },
    ]);
    expect(new TextDecoder().decode(socket.sent.at(-1)!.payload)).toBe(
      '{"type":"hello","client_id":"android"}',
    );
    socket.receive({ t: "accept", seq: 2, s: transport.id }, '{"type":"server_info"}');
    expect(events.message).toHaveBeenCalledExactlyOnceWith('{"type":"server_info"}', false);
    // Frames of other sessions are not delivered here.
    socket.receive({ t: "msg", seq: 3, s: "other", pt: "text" }, "x");
    expect(events.message).toHaveBeenCalledOnce();
  });

  it("maps a center close to transport error and close events without echoing it", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    transport.send('{"type":"hello"}');
    socket.receive({ t: "close", seq: 2, s: transport.id, code: "peer_gone" });
    expect(events.error).toHaveBeenCalledWith(new HubSessionClosedError("peer_gone"));
    expect(events.error.mock.calls[0][0]).toMatchObject({
      code: "peer_gone",
      message: "Hub session closed (peer_gone).",
    });
    expect(events.close).toHaveBeenCalledWith({
      code: 1006,
      reason: "Hub session closed (peer_gone).",
      hubCode: "peer_gone",
    });
    expect(transport.closeCode).toBe("peer_gone");
    expect(socket.frames("close")).toEqual([]);
    expect(() => transport.send("late")).toThrow("closed");
  });

  it("opens guest sessions with the share ID only when one is given", async () => {
    const h = hubFixture();
    h.link.start();
    await h.flush();
    const socket = h.socket();
    socket.open();
    socket.welcome({ center_methods: ["share.subscribe"] });
    const own = h.link.openSession(HOST.node_id);
    const guest = h.link.openSession("guest-node", { share: "share-1" });
    expect(guest.share).toBe("share-1");
    const close = vi.fn();
    guest.onClose(close);
    await h.flush();
    own.send('{"type":"hello"}');
    guest.send('{"type":"hello"}');
    expect(socket.frames("open")).toEqual([
      { t: "open", seq: 1, s: own.id, to: { node: HOST.node_id } },
      { t: "open", seq: 2, s: guest.id, to: { node: "guest-node" }, share: "share-1" },
    ]);
    expect(own.closeCode).toBeNull();
    socket.receive({ t: "close", seq: 1, s: guest.id, code: "share_revoked" });
    expect(guest.closeCode).toBe("share_revoked");
    expect(close).toHaveBeenCalledWith(expect.objectContaining({ hubCode: "share_revoked" }));
    expect(socket.frames("close")).toEqual([]);
  });

  it("refuses guest sessions on centers without shares instead of sending the field", async () => {
    const { h, socket } = await session();
    expect(() => h.link.openSession("guest-node", { share: "share-1" })).toThrow(
      expect.objectContaining({ code: "unsupported" }),
    );
    expect(socket.frames("open")).toEqual([]);
  });

  it("does not report a hub code for local failures", async () => {
    const { h, transport, events } = await session();
    await h.flush();
    transport.abort(new Error("local"));
    expect(events.close).toHaveBeenCalledWith({ code: 1006, reason: "local" });
    expect(transport.closeCode).toBeNull();
  });

  it("closes quietly on a business close and tells the peer once opened", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    transport.send('{"type":"hello"}');
    transport.close();
    transport.close();
    expect(socket.frames("close")).toEqual([
      { t: "close", seq: 2, s: transport.id, code: "closed" },
    ]);
    expect(events.close).not.toHaveBeenCalled();

    const unopened = h.link.openSession(HOST.node_id);
    unopened.close();
    expect(socket.frames("close")).toHaveLength(1);
  });

  it("fails sessions the Host never accepts", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    transport.send('{"type":"hello"}');
    await vi.advanceTimersByTimeAsync(20_000);
    socket.receive({ t: "pong", n: socket.frames("ping")[0].n });
    await vi.advanceTimersByTimeAsync(10_000);
    expect(events.error).toHaveBeenCalledWith(new Error("Hub session was not accepted in time."));
    expect(events.close).toHaveBeenCalledOnce();
    expect(socket.frames("close")).toEqual([
      { t: "close", seq: 2, s: transport.id, code: "failed" },
    ]);
  });

  it("rejects oversized frames instead of silently losing input", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    transport.send('{"type":"hello"}');
    socket.receive({ t: "accept", seq: 2, s: transport.id });
    expect(events.message).not.toHaveBeenCalled();
    expect(() => transport.send(new Uint8Array(1024 * 1024 + 1))).toThrow("too large");
    expect(events.close).toHaveBeenCalledOnce();
    expect(socket.frames("msg")).toEqual([]);
  });

  it("aborts on an accept it never asked for and tells the peer", async () => {
    const { h, socket, transport, events } = await session();
    await h.flush();
    transport.send('{"type":"hello"}');
    socket.receive({ t: "accept", seq: 2, s: transport.id });
    socket.receive({ t: "accept", seq: 3, s: transport.id });
    expect(events.error).toHaveBeenCalledWith(new Error("Unexpected hub session accept."));
    expect(socket.frames("close")).toEqual([
      { t: "close", seq: 2, s: transport.id, code: "failed" },
    ]);
    expect(transport.closed).toBe(true);
    expect(() => transport.send("late")).toThrow("Hub session is closed.");
  });

  it("stops delivering events to handlers that unsubscribed", async () => {
    const h = hubFixture();
    const socket = await h.online();
    const transport = h.link.openSession(HOST.node_id);
    const handlers = { open: vi.fn(), close: vi.fn(), error: vi.fn(), message: vi.fn() };
    const removers = [
      transport.onOpen(handlers.open),
      transport.onClose(handlers.close),
      transport.onError(handlers.error),
      transport.onMessage(handlers.message),
    ];
    for (const remove of removers) remove();
    await h.flush();
    transport.send('{"type":"hello"}');
    socket.receive({ t: "accept", seq: 2, s: transport.id }, "info");
    socket.receive({ t: "close", seq: 3, s: transport.id, code: "done" });
    for (const handler of Object.values(handlers)) expect(handler).not.toHaveBeenCalled();
  });
});
