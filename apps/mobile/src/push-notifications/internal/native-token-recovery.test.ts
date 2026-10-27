import { afterEach, expect, it, vi } from "vitest";
import { startSubscription } from "./subscriptions";

const mocks = vi.hoisted(() => ({
  nativeToken: vi.fn(),
  register: vi.fn(),
}));

vi.mock("react-native", () => ({
  Platform: { OS: "ios" },
  AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
}));
vi.mock("expo-modules-core", () => ({ Platform: { OS: "ios" } }));
vi.mock("expo-notifications/build/PushTokenManager", () => ({
  default: { getDevicePushTokenAsync: mocks.nativeToken },
}));
vi.mock("expo-notifications/build/warnOfExpoGoPushUsage", () => ({
  warnOfExpoGoPushUsage() {},
}));
vi.mock("expo-notifications", async () => {
  const { default: getDevicePushTokenAsync } =
    await import("expo-notifications/build/getDevicePushTokenAsync.js");
  return { getDevicePushTokenAsync, addPushTokenListener: () => ({ remove() {} }) };
});
vi.mock("expo-application", () => ({
  applicationId: "com.necokeine.ait",
  getIosPushNotificationServiceEnvironmentAsync: async () => "production",
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {} },
}));
vi.mock("./permissions", () => ({ ensureNativeNotificationPermission: async () => true }));

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

it("keeps concurrent native lookups shared and releases a successful request", async () => {
  const { default: getDevicePushTokenAsync } =
    await import("expo-notifications/build/getDevicePushTokenAsync.js");
  let complete: (token: string) => void = () => {};
  const pending = new Promise<string>((resolve) => {
    complete = resolve;
  });
  mocks.nativeToken.mockReturnValueOnce(pending).mockResolvedValue("next-token");
  const first = getDevicePushTokenAsync();
  const second = getDevicePushTokenAsync();
  expect(mocks.nativeToken).toHaveBeenCalledOnce();
  complete("shared-token");
  await expect(Promise.all([first, second])).resolves.toEqual([
    { type: "ios", data: "shared-token" },
    { type: "ios", data: "shared-token" },
  ]);
  await expect(getDevicePushTokenAsync()).resolves.toEqual({ type: "ios", data: "next-token" });
  expect(mocks.nativeToken).toHaveBeenCalledTimes(2);
});

it("registers after a transient native token failure using Expo's real token lookup", async () => {
  vi.useFakeTimers();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  mocks.nativeToken
    .mockRejectedValueOnce(new Error("temporary APNs registration failure"))
    .mockResolvedValue("a".repeat(64));
  const stop = startSubscription({
    serverId: "test-host",
    client: {
      isConnected: true,
      registerPushToken: mocks.register,
      unregisterPushToken: async () => {},
      subscribeConnectionStatus: () => () => {},
    },
  });
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.register).not.toHaveBeenCalled();
    expect(mocks.nativeToken).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mocks.register).toHaveBeenCalledWith(
      JSON.stringify({
        provider: "apns",
        token: "a".repeat(64),
        bundleId: "com.necokeine.ait",
        environment: "production",
      }),
    );
    expect(mocks.nativeToken).toHaveBeenCalledTimes(2);
  } finally {
    stop();
  }
});
