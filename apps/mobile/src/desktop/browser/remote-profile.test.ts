import { beforeEach, expect, test, vi } from "vitest";
import { resolveBrowserProfileHost } from "./remote-profile";

const state = vi.hoisted(() => ({
  connection: {
    id: "relay",
    type: "accountRelay",
    hostId: "remote-host",
    center: "https://center.test",
  },
  activeId: "relay",
}));
vi.mock("@/runtime/host-runtime", () => ({
  getHostRuntimeStore: () => ({
    isHostRegistryLoaded: () => true,
    getSnapshot: () => ({ activeConnectionId: state.activeId }),
    getHosts: () => [
      {
        serverId: "remote-server",
        preferredConnectionId: "relay",
        connections: [state.connection],
      },
    ],
  }),
}));

beforeEach(() => {
  state.connection.type = "accountRelay";
});

test("configures the Workspace's relay profile before a guest can navigate", async () => {
  const registerAttachedBrowser = vi.fn();
  let complete: ((profile: { profilePartition: string }) => void) | undefined;
  const prepareAccountProfile = vi.fn(
    () =>
      new Promise<{ profilePartition: string }>((resolve) => {
        complete = resolve;
      }),
  );
  let finished = false;
  const resolution = resolveBrowserProfileHost("remote-server", {
    prepareAccountProfile,
    registerAttachedBrowser,
  }).then((profile) => {
    finished = true;
    return profile;
  });
  await vi.waitFor(() =>
    expect(prepareAccountProfile).toHaveBeenCalledWith({
      serverId: "remote-server",
      hostId: "remote-host",
      center: "https://center.test",
    }),
  );
  expect(finished).toBe(false);
  complete?.({ profilePartition: "persist:remote-profile" });
  expect(await resolution).toEqual({
    profilePartition: "persist:remote-profile",
    registerAttachedBrowser,
  });
});

test("leaves direct and built-in hosts on the normal browser profile", async () => {
  state.connection.type = "directTcp";
  const prepareAccountProfile = vi.fn();
  expect(
    await resolveBrowserProfileHost("remote-server", {
      prepareAccountProfile,
      registerAttachedBrowser: vi.fn(),
    }),
  ).toBeUndefined();
  expect(prepareAccountProfile).not.toHaveBeenCalled();
});

test("fails instead of falling back to client localhost when remote routing fails", async () => {
  const prepareAccountProfile = vi.fn().mockRejectedValue(new Error("relay profile failed"));
  await expect(
    resolveBrowserProfileHost("remote-server", {
      prepareAccountProfile,
      registerAttachedBrowser: vi.fn(),
    }),
  ).rejects.toThrow("relay profile failed");
});
