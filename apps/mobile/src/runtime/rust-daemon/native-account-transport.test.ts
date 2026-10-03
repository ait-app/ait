import { afterEach, describe, expect, it, vi } from "vitest";
import { createNativeAccountRelayTransportFactory } from "./native-account-transport";
import { nativeRelayHarness } from "./native-relay-test-harness";
import { createRustDaemonTransportFactory } from "./transport";

const url = "ait+desktop://account-relay/11111111-1111-4111-8111-111111111111";
afterEach(() => vi.useRealTimers());

describe("native account relay", () => {
  it("negotiates one physical Rust connection and exchanges SDK ping messages", async () => {
    const h = nativeRelayHarness();
    const factory = createRustDaemonTransportFactory(
      createNativeAccountRelayTransportFactory(h.deps),
    );
    const transport = factory({ url });
    const receive = vi.fn();
    const error = vi.fn();
    transport.onMessage(receive);
    transport.onError(error);
    transport.onOpen(() => transport.send(JSON.stringify({ type: "hello", clientId: "android" })));
    await h.flush();
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    const hello = JSON.parse(String(vi.mocked(h.socket.send).mock.calls[0]![0]));
    expect(hello).toMatchObject({
      type: "hello",
      client_id: "android",
      required_capabilities: ["connection.single.v1"],
    });
    h.message({
      type: "server_info",
      info: {
        server_id: "server",
        instance_id: "instance",
        features: ["ait-rust-single-v1"],
        protocol: { major: 1, minor: 0 },
        implemented_capabilities: ["connection.ping"],
      },
      negotiated_capabilities: ["connection.ping"],
    });
    transport.send('{"type":"ping"}');
    const ping = JSON.parse(String(vi.mocked(h.socket.send).mock.calls.at(-1)![0]));
    expect(ping).toMatchObject({ type: "request", method: "connection.ping" });
    h.message({ type: "response", request_id: ping.request_id, result: {} });
    expect(receive).toHaveBeenLastCalledWith('{"type":"pong"}', false);
    expect(h.deps.connect).toHaveBeenCalledOnce();
    expect(error).not.toHaveBeenCalled();
    transport.close();
  });

  it("opens only after pairing, validates the runtime and carries terminal/file binary frames", async () => {
    const h = nativeRelayHarness();
    const transport = createNativeAccountRelayTransportFactory(h.deps)({ url });
    const open = vi.fn(() => transport.send(JSON.stringify({ type: "hello" })));
    const receive = vi.fn();
    transport.onOpen(open);
    transport.onMessage(receive);
    await h.flush();
    expect(h.deps.connect).toHaveBeenCalledWith({
      url: h.grant.url,
      headers: { Authorization: "Bearer private-ticket" },
    });
    expect(open).not.toHaveBeenCalled();
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    expect(open).toHaveBeenCalledOnce();
    h.message({
      type: "server_info",
      info: { server_id: "server", instance_id: "instance", features: ["ait-rust-single-v1"] },
    });
    const frame = new Uint8Array([0x10, 5, 7]);
    h.message(frame, true);
    expect(receive).toHaveBeenLastCalledWith(frame, true);
    transport.send(frame);
    expect(h.socket.send).toHaveBeenLastCalledWith(frame);
    transport.close();
    expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
    expect(h.remove).toHaveBeenCalledOnce();
  });

  it.each(["pairing", "identity"])("rejects a mismatched %s", async (kind) => {
    const h = nativeRelayHarness();
    const transport = createNativeAccountRelayTransportFactory(h.deps)({ url });
    const error = vi.fn();
    transport.onError(error);
    transport.onOpen(() => transport.send('{"type":"hello"}'));
    await h.flush();
    h.message({ type: "relay.ready", relay_session_id: kind === "pairing" ? "wrong" : "visit" });
    if (kind === "identity")
      h.message({
        type: "server_info",
        info: { server_id: "wrong", instance_id: "instance", features: ["ait-rust-single-v1"] },
      });
    expect(error).toHaveBeenCalledOnce();
    expect(h.socket.close).toHaveBeenCalledOnce();
    expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
  });

  it("releases a late ticket after logout without opening its socket", async () => {
    const h = nativeRelayHarness();
    let resolve!: (grant: typeof h.grant) => void;
    h.account.openVisit.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const transport = createNativeAccountRelayTransportFactory(h.deps)({ url });
    const close = vi.fn();
    transport.onClose(close);
    await h.flush();
    h.cancel();
    resolve(h.grant);
    await h.flush();
    expect(h.deps.connect).not.toHaveBeenCalled();
    expect(h.account.closeVisit).toHaveBeenCalledWith("visit");
    expect(close).toHaveBeenCalledOnce();
  });

  it("fails pairing timeouts", async () => {
    vi.useFakeTimers();
    const h = nativeRelayHarness();
    const transport = createNativeAccountRelayTransportFactory(h.deps)({ url });
    const error = vi.fn();
    transport.onError(error);
    await h.flush();
    await vi.advanceTimersByTimeAsync(45_000);
    expect(error).toHaveBeenCalledOnce();
    expect(h.socket.close).toHaveBeenCalledOnce();
  });

  it("closes an oversized outgoing frame instead of silently losing input", async () => {
    const h = nativeRelayHarness();
    const transport = createNativeAccountRelayTransportFactory(h.deps)({ url });
    await h.flush();
    h.message({ type: "relay.ready", relay_session_id: "visit" });
    expect(() => transport.send(new Uint8Array(1024 * 1024 + 1))).toThrow("too large");
    expect(h.socket.send).not.toHaveBeenCalled();
    expect(h.socket.close).toHaveBeenCalledOnce();
  });
});
