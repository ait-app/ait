import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { startPushNotifications } from "../index.native";
import { refreshPushRegistrations } from "./subscriptions";

const mocks = vi.hoisted(() => ({
  platform: { OS: "ios" },
  appState: { currentState: "active", addEventListener: vi.fn() },
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  permissions: vi.fn(),
  request: vi.fn(),
  deviceToken: vi.fn(),
  channel: vi.fn(),
  tokenListener: vi.fn(),
  environment: vi.fn(),
}));

vi.mock("react-native", () => ({ Platform: mocks.platform, AppState: mocks.appState }));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: mocks.getItem, setItem: mocks.setItem, removeItem: mocks.removeItem },
}));
vi.mock("expo-application", () => ({
  applicationId: "dev.ait.mobile",
  getIosPushNotificationServiceEnvironmentAsync: mocks.environment,
}));
vi.mock("expo-notifications", () => ({
  PermissionStatus: { GRANTED: "granted" },
  IosAuthorizationStatus: { PROVISIONAL: 3, EPHEMERAL: 4 },
  AndroidImportance: { DEFAULT: 3 },
  getPermissionsAsync: mocks.permissions,
  requestPermissionsAsync: mocks.request,
  getDevicePushTokenAsync: mocks.deviceToken,
  setNotificationChannelAsync: mocks.channel,
  addPushTokenListener: mocks.tokenListener,
}));

function client() {
  return {
    isConnected: true,
    registerPushToken: vi.fn((_token: string) => {}),
    unregisterPushToken: vi.fn(async (_token: string) => {}),
    subscribeConnectionStatus: vi.fn(() => vi.fn()),
  };
}
const cleanups: (() => void)[] = [];
function start() {
  const host = client();
  cleanups.push(
    startPushNotifications({ client: host as unknown as DaemonClient, serverId: "host" }),
  );
  return host;
}
const granted = { status: "granted", granted: true, canAskAgain: true };

describe("native push registration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.platform.OS = "ios";
    mocks.appState.currentState = "active";
    mocks.appState.addEventListener.mockReturnValue({ remove: vi.fn() });
    mocks.tokenListener.mockReturnValue({ remove: vi.fn() });
    mocks.getItem.mockResolvedValue(null);
    mocks.setItem.mockResolvedValue(undefined);
    mocks.removeItem.mockResolvedValue(undefined);
    mocks.permissions.mockResolvedValue({
      status: "undetermined",
      granted: false,
      canAskAgain: true,
    });
    mocks.request.mockResolvedValue(granted);
    mocks.deviceToken.mockResolvedValue({ type: "ios", data: "native-device" });
    mocks.environment.mockResolvedValue("production");
    mocks.channel.mockResolvedValue(null);
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    cleanups.splice(0).forEach((stop) => stop());
    vi.restoreAllMocks();
  });

  it("requests system permission even with a broken cache and registers APNs without an Expo project", async () => {
    mocks.getItem.mockRejectedValue(new Error("test storage failure"));
    mocks.setItem.mockRejectedValue(new Error("test storage failure"));
    const host = start();
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledTimes(1));
    expect(mocks.request).toHaveBeenCalledWith({
      ios: { allowAlert: true, allowBadge: true, allowSound: true },
    });
    expect(JSON.parse(host.registerPushToken.mock.calls[0][0])).toEqual({
      provider: "apns",
      token: "native-device",
      bundleId: "dev.ait.mobile",
      environment: "production",
    });
  });

  it("creates the Android channel before requesting permission and registers a native FCM token", async () => {
    mocks.platform.OS = "android";
    const host = start();
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledOnce());
    expect(mocks.channel.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.request.mock.invocationCallOrder[0],
    );
    expect(JSON.parse(host.registerPushToken.mock.calls[0][0])).toEqual({
      provider: "fcm",
      token: "native-device",
    });
    expect(mocks.environment).not.toHaveBeenCalled();
  });

  it("waits until foreground to request permission and recovers when returning from system settings", async () => {
    mocks.appState.currentState = "background";
    const host = start();
    expect(mocks.request).not.toHaveBeenCalled();
    mocks.appState.currentState = "active";
    const listener = mocks.appState.addEventListener.mock.calls[0][1];
    listener("active");
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledOnce());
    mocks.deviceToken.mockResolvedValue({ type: "ios", data: "rotated-device" });
    listener("active");
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledTimes(2));
    expect(host.unregisterPushToken).toHaveBeenCalledTimes(1);
  });

  it("honors denied permission and refreshes registration after explicitly enabling permission", async () => {
    mocks.permissions.mockResolvedValue({ status: "denied", granted: false, canAskAgain: false });
    const host = start();
    await vi.waitFor(() => expect(mocks.removeItem).toHaveBeenCalled());
    expect(mocks.request).not.toHaveBeenCalled();
    expect(host.registerPushToken).not.toHaveBeenCalled();
    mocks.permissions.mockResolvedValue(granted);
    refreshPushRegistrations();
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledOnce());
  });

  it("shares one permission prompt across hosts, accepts provisional permission, and stops cleanly", async () => {
    const first = start();
    const second = start();
    await vi.waitFor(() => expect(second.registerPushToken).toHaveBeenCalledOnce());
    expect(first.registerPushToken).toHaveBeenCalledOnce();
    expect(mocks.request).toHaveBeenCalledOnce();
    cleanups.splice(0).forEach((stop) => stop());
    mocks.request.mockClear();
    mocks.permissions.mockResolvedValue({
      status: "undetermined",
      granted: false,
      canAskAgain: true,
      ios: { status: 3 },
    });
    const provisional = start();
    await vi.waitFor(() => expect(provisional.registerPushToken).toHaveBeenCalledOnce());
    expect(mocks.request).not.toHaveBeenCalled();
    cleanups.splice(0).forEach((stop) => stop());
    refreshPushRegistrations();
    expect(provisional.registerPushToken).toHaveBeenCalledOnce();
  });

  it("uses the rotated native token directly without recursively requesting another token", async () => {
    const host = start();
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledOnce());
    mocks.deviceToken.mockClear();
    const changed = mocks.tokenListener.mock.calls[0][0];
    changed({ type: "ios", data: "rotated-native-token" });
    await vi.waitFor(() => expect(host.registerPushToken).toHaveBeenCalledTimes(2));
    expect(mocks.deviceToken).not.toHaveBeenCalled();
    expect(JSON.parse(host.registerPushToken.mock.calls[1][0]).token).toBe("rotated-native-token");
  });
});
