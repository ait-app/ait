import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  bridge: null as null | {
    invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
    window: {
      openNew(options?: { workspace?: { serverId: string; workspaceId: string } }): Promise<void>;
    };
  },
  invoke: vi.fn(),
}));

vi.mock("electron", () => ({
  contextBridge: {
    exposeInMainWorld: (_name: string, bridge: NonNullable<typeof mocks.bridge>) => {
      mocks.bridge = bridge;
    },
  },
  ipcRenderer: { invoke: mocks.invoke },
  webUtils: {},
}));

beforeEach(() => {
  vi.resetModules();
  mocks.invoke.mockReset();
  mocks.bridge = null;
});

describe("desktop command bridge", () => {
  it("sends host synchronization through the Ait IPC channel", async () => {
    await import("./preload.js");
    const args = { serverId: "host", instanceId: "instance", needsGrant: true };
    mocks.invoke.mockResolvedValue({ node_session_id: "lease" });
    await expect(mocks.bridge?.invoke("account_host_sync", args)).resolves.toEqual({
      node_session_id: "lease",
    });
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("ait:invoke", "account_host_sync", args);
  });

  it("forwards the workspace identity when opening another application window", async () => {
    await import("./preload.js");
    const options = { workspace: { serverId: "remote-host", workspaceId: "workspace-42" } };
    mocks.invoke.mockResolvedValue(undefined);
    await mocks.bridge?.window.openNew(options);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("paseo:window:openNew", options);
  });
});
