import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAccountRelayTransportFactory } from "./account-transport";

const mocks = vi.hoisted(() => ({
  platform: "android",
  connect: vi.fn(),
}));

vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return mocks.platform;
    },
  },
}));
vi.mock("@/desktop/host", () => ({ getDesktopHost: () => null }));
vi.mock("./native-account-transport", () => ({
  createNativeAccountRelayTransportFactory: () => mocks.connect,
}));

beforeEach(() => vi.clearAllMocks());

describe("native mobile account transport selection", () => {
  it.each(["android", "ios"])(
    "routes %s account relays through native ticket transport",
    (platform) => {
      mocks.platform = platform;
      const target = { url: "ait+desktop://account-relay/11111111-1111-4111-8111-111111111111" };
      createAccountRelayTransportFactory(target);
      expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(target);
    },
  );
});
