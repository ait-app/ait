import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAccountRelayTransportFactory } from "./account-transport";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(async () => {}),
  on: vi.fn(async () => () => {}),
}));
vi.mock("react-native", () => ({ Platform: { OS: "web" } }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => ({ invoke: mocks.invoke, events: { on: mocks.on } }),
}));
vi.mock("./native-account-transport", () => ({
  createNativeAccountRelayTransportFactory: vi.fn(),
}));
beforeEach(() => vi.clearAllMocks());

describe("desktop saved relay targets", () => {
  it("passes the persisted service address to the main-process ticket authority", async () => {
    const transport = createAccountRelayTransportFactory({
      url: "ait+desktop://account-relay/11111111-1111-4111-8111-111111111111?center=https%3A%2F%2Fcustom.test%3A9443%2Fait%2Fapi",
    });
    await vi.waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("account_transport_open", {
        sessionId: expect.stringMatching(/^account-/),
        hostId: "11111111-1111-4111-8111-111111111111",
        center: "https://custom.test:9443/ait/api",
      }),
    );
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
