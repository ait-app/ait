import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAccountRelayTransportFactory } from "./account-transport";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  on: vi.fn<(event: string, handler: (payload: unknown) => void) => Promise<() => void>>(),
}));
const removeListener = vi.fn();
const target = {
  url: "ait+desktop://account-relay/11111111-1111-4111-8111-111111111111?center=https%3A%2F%2Fcustom.test%3A9443%2Fait%2Fapi",
};
vi.mock("react-native", () => ({ Platform: { OS: "web" } }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => ({ invoke: mocks.invoke, events: { on: mocks.on } }),
}));
beforeEach(() => {
  vi.resetAllMocks();
  mocks.invoke.mockResolvedValue(undefined);
  mocks.on.mockResolvedValue(removeListener);
});

async function openDesktopTransport() {
  const transport = createAccountRelayTransportFactory(target);
  await vi.waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("account_transport_open", expect.anything()),
  );
  const sessionId = mocks.invoke.mock.calls[0][1]?.sessionId;
  const receive = mocks.on.mock.calls[0][1];
  return {
    transport,
    emit: (payload: Record<string, unknown>) => receive({ sessionId, ...payload }),
  };
}

describe("desktop saved relay targets", () => {
  it("passes the persisted service address to the main-process ticket authority", async () => {
    const { transport } = await openDesktopTransport();
    expect(mocks.invoke).toHaveBeenCalledWith("account_transport_open", {
      sessionId: expect.stringMatching(/^account-/),
      hostId: "11111111-1111-4111-8111-111111111111",
      center: "https://custom.test:9443/ait/api",
    });
    transport.close();
  });

  it("rejects a target without a service binding before invoking main-process account access", () => {
    expect(() =>
      createAccountRelayTransportFactory({
        url: "ait+desktop://account-relay/11111111-1111-4111-8111-111111111111",
      }),
    ).toThrow("Invalid account relay target.");
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});

describe("desktop relay closure", () => {
  it("keeps the peer's close reason when pending IPC calls settle late", async () => {
    const { transport, emit } = await openDesktopTransport();
    const error = vi.fn();
    const close = vi.fn(() => transport.close());
    const message = vi.fn();
    transport.onError(error);
    transport.onClose(close);
    transport.onMessage(message);
    const ack = Promise.withResolvers<void>();
    const send = Promise.withResolvers<void>();
    const probe = Promise.withResolvers<boolean>();
    mocks.invoke
      .mockReturnValueOnce(ack.promise)
      .mockReturnValueOnce(send.promise)
      .mockReturnValueOnce(probe.promise);

    emit({ kind: "message", text: "response", sequence: 1 });
    transport.send("request");
    const recovery = transport.recoverLiveness?.();
    emit({ kind: "close", error: "Hub session failed" });
    emit({ kind: "close", error: "Duplicate close" });
    emit({ kind: "message", text: "late frame", sequence: 2 });
    const staleError = new Error("Relay transport is closed");
    ack.reject(staleError);
    send.reject(staleError);
    probe.reject(staleError);

    expect(await recovery).toBe(false);
    expect(await transport.recoverLiveness?.()).toBe(false);
    expect(message).toHaveBeenCalledExactlyOnceWith("response", false);
    expect(close).toHaveBeenCalledExactlyOnceWith({ code: 1006, reason: "Hub session failed" });
    expect(removeListener).toHaveBeenCalledTimes(1);
    expect(error).not.toHaveBeenCalled();
    expect(mocks.invoke.mock.calls.map(([command]) => command)).toEqual([
      "account_transport_open",
      "account_transport_ack",
      "account_transport_send",
      "account_transport_recover",
    ]);
  });

  it("skips the ACK when a message handler closes the transport", async () => {
    const { transport, emit } = await openDesktopTransport();
    transport.onMessage(() => transport.close());

    emit({ kind: "message", text: "response", sequence: 1 });

    expect(removeListener).toHaveBeenCalledTimes(1);
    expect(mocks.invoke.mock.calls.map(([command]) => command)).toEqual([
      "account_transport_open",
      "account_transport_close",
    ]);
  });

  it("reports a send failure while the transport is live", async () => {
    const { transport } = await openDesktopTransport();
    const error = vi.fn();
    transport.onError(error);
    const failure = new Error("Hub frame too large");
    mocks.invoke.mockRejectedValueOnce(failure);

    transport.send("request");

    await vi.waitFor(() => expect(error).toHaveBeenCalledExactlyOnceWith(failure));
    transport.close();
  });
});
