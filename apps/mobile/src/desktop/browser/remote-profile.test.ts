import { beforeEach, expect, test, vi } from "vitest";
import type { HostConnection } from "@/types/host-connection";
import { resolveBrowserProfileHost } from "./remote-profile";

const state = vi.hoisted(() => ({
  connection: {
    id: "relay",
    type: "accountRelay",
    hostId: "remote-host",
    center: "https://center.test",
  } as HostConnection,
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
  state.connection = {
    id: "relay",
    type: "accountRelay",
    hostId: "remote-host",
    center: "https://center.test",
  };
});

test("configures the Workspace's relay profile before a guest can navigate", async () => {
  const registerAttachedBrowser = vi.fn();
  let complete: ((profile: { profilePartition: string }) => void) | undefined;
  const prepareWorkspaceProfile = vi.fn(
    () =>
      new Promise<{ profilePartition: string }>((resolve) => {
        complete = resolve;
      }),
  );
  let finished = false;
  const resolution = resolveBrowserProfileHost("remote-server", {
    prepareWorkspaceProfile,
    registerAttachedBrowser,
  }).then((profile) => {
    finished = true;
    return profile;
  });
  await vi.waitFor(() =>
    expect(prepareWorkspaceProfile).toHaveBeenCalledWith({
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

test.each([
  [
    { id: "tcp", type: "directTcp", endpoint: "192.168.1.2:7316", password: "token" },
    { transportType: "rustTcp", url: "ws://192.168.1.2:7316/v1/ws" },
  ],
  [
    {
      id: "ssh",
      type: "remoteSsh",
      host: "user@host",
      sshPort: 22,
      daemonPort: 7316,
      password: "token",
    },
    { transportType: "ssh", host: "user@host", sshPort: 22, daemonPort: 7316 },
  ],
] as const)(
  "prepares daemon forwarding for $0.type instead of using the local browser route",
  async (connection, target) => {
    state.connection = connection;
    const prepareWorkspaceProfile = vi
      .fn()
      .mockResolvedValue({ profilePartition: "persist:remote-profile" });
    expect(
      await resolveBrowserProfileHost("remote-server", {
        prepareWorkspaceProfile,
        registerAttachedBrowser: vi.fn(),
      }),
    ).toHaveProperty("profilePartition", "persist:remote-profile");
    expect(prepareWorkspaceProfile).toHaveBeenCalledWith({
      serverId: "remote-server",
      target,
      bearerToken: "token",
    });
  },
);

test("fails instead of falling back to client localhost when remote routing fails", async () => {
  const prepareWorkspaceProfile = vi.fn().mockRejectedValue(new Error("relay profile failed"));
  await expect(
    resolveBrowserProfileHost("remote-server", {
      prepareWorkspaceProfile,
      registerAttachedBrowser: vi.fn(),
    }),
  ).rejects.toThrow("relay profile failed");
});
