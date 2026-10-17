/** @vitest-environment jsdom */
import "fake-indexeddb/auto";
import { afterEach, expect, it, vi } from "vitest";
import { AccountSessionManager, type AccountHost } from "@ait/client/internal/account-session";
import { channelFixture, welcome } from "../../../../packages/client/test/account-channel";
import { HostRuntimeController, HostRuntimeStore } from "./host-runtime";
import { accountCommand, useAccountState } from "./account-state";

const runtime = vi.hoisted(() => ({
  store: null as HostRuntimeStore | null,
  manager: null as AccountSessionManager | null,
}));
vi.mock("@/desktop/host", async (original) => ({
  ...(await original<typeof import("@/desktop/host")>()),
  isElectronRuntime: () => false,
  getDesktopHost: () => ({ invoke: async () => runtime.manager!.snapshot() }),
}));
vi.mock("./native-account", async (original) => ({
  ...(await original<typeof import("./native-account")>()),
  getNativeAccount: vi.fn(),
  subscribeNativeAccount: vi.fn(),
}));
vi.mock("./browser-account", async (original) => ({
  ...(await original<typeof import("./browser-account")>()),
  supportsBrowserAccount: () => false,
  getBrowserAccount: vi.fn(),
  subscribeBrowserAccount: vi.fn(),
}));
vi.mock("./host-runtime", async (original) => ({
  ...(await original<typeof import("./host-runtime")>()),
  getHostRuntimeStore: () => runtime.store!,
}));

afterEach(async () => {
  await runtime.manager?.logout();
  runtime.manager = null;
  runtime.store = null;
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it.each(["initial connection", "reconnection"])(
  "imports and requests a connection for an online event before the first snapshot: %s",
  async (scenario) => {
    // Exercise real discovery persistence and availability dispatch, stopping at the network boundary.
    vi.spyOn(HostRuntimeController.prototype, "start").mockResolvedValue();
    vi.spyOn(HostRuntimeController.prototype, "updateHost").mockResolvedValue();
    const attempts: string[] = [];
    vi.spyOn(HostRuntimeController.prototype, "ensureConnected").mockImplementation(
      function (this: HostRuntimeController) {
        attempts.push(this.getSnapshot().serverId);
      },
    );
    const storage = new Map([["@paseo:e2e", "1"]]);
    const store = new HostRuntimeStore({
      storage: {
        getItem: async (key) => storage.get(key) ?? null,
        setItem: async (key, value) => {
          storage.set(key, value);
        },
        removeItem: async (key) => {
          storage.delete(key);
        },
      },
      revokePushNotifications: vi.fn(async () => {}),
    });
    runtime.store = store;
    await store.boot();
    vi.useFakeTimers();
    const first: AccountHost = {
      host_id: "11111111-1111-4111-8111-111111111111",
      node_id: "first-node",
      server_id: "first-server",
      instance_id: "new-instance",
      name: "First",
      platform: "linux",
      online: true,
      status: "offline",
      relay_modes: ["ait-rust-single-v1"],
    };
    const second: AccountHost = {
      ...first,
      host_id: "22222222-2222-4222-8222-222222222222",
      node_id: "second-node",
      server_id: "second-server",
      name: "Second",
    };
    const channel = channelFixture(() => [], false);
    const manager = new AccountSessionManager({
      installationId: "client-installation",
      deviceName: "Client",
      platform: "web",
      appVersion: "test",
      randomUUID: () => crypto.randomUUID(),
      runtime: () => ({ status: "stopped", serverId: "" }),
      local: async () => undefined,
      save: async () => {},
      notify: () => {},
      closeTransports: () => {},
      channelTransport: channel.create,
      fetch: async (input) => {
        if (String(input).endsWith("/nodes/register"))
          return Response.json({
            node_id: "client",
            node_session_id: "client-session",
            host_id: null,
            control_required: false,
          });
        if (String(input).endsWith("/channel-tickets"))
          return Response.json({
            channel_ticket: "one-use-ticket",
            channel_path: "/v1/relay/channel",
            protocol: "ait-channel-v1",
          });
        return new Response(null, { status: 204 });
      },
    });
    runtime.manager = manager;
    await manager.restore({
      center: "https://center.test/api",
      token: "test-token",
      name: "Me",
      expiresAt: Date.now() + 3600_000,
    });
    await vi.advanceTimersByTimeAsync(1);
    let socket = channel.sockets[0]!;
    const fullSnapshot = { type: "hosts.snapshot", hosts: [first, second], observed_at: "now" };
    if (scenario === "reconnection") {
      socket.receive(welcome);
      socket.receive(fullSnapshot);
      await accountCommand("account_status");
      socket.disconnect();
      await accountCommand("account_status");
      await vi.advanceTimersByTimeAsync(4000);
      expect(channel.sockets).toHaveLength(2);
      socket = channel.sockets[1]!;
    }
    await accountCommand("account_status");
    attempts.length = 0;
    socket.receive(welcome);
    socket.receive({
      type: "node.online",
      node_id: first.node_id,
      node_session_id: "host-session",
      host_id: first.host_id,
      host: first,
      observed_at: "now",
    });
    await accountCommand("account_status");
    expect(manager.snapshot()).toMatchObject({
      status: "connecting",
      stale: true,
      confirmedHostIds: [first.host_id],
    });
    expect(useAccountState.getState().stale).toBe(true);
    expect(store.getHosts().some((host) => host.serverId === first.server_id)).toBe(true);
    expect(attempts).toEqual([first.server_id]);
    if (scenario === "reconnection") {
      expect(manager.snapshot().hosts.find((host) => host.host_id === second.host_id)).toEqual(
        second,
      );
      expect(store.getHosts()).toHaveLength(2);
    } else {
      expect(store.getHosts()).toHaveLength(1);
    }
    // Repeated events do not restart an already available Host.
    socket.receive({
      type: "node.online",
      node_id: first.node_id,
      node_session_id: "host-session",
      host_id: first.host_id,
      host: first,
      observed_at: "now",
    });
    await accountCommand("account_status");
    expect(attempts).toEqual([first.server_id]);
    socket.receive({
      type: "node.offline",
      node_id: first.node_id,
      node_session_id: "host-session",
      host_id: first.host_id,
      observed_at: "later",
    });
    await accountCommand("account_status");
    expect(manager.snapshot().confirmedHostIds).toEqual([]);
    // The full snapshot confirms the remaining cache; only now may the second Host connect.
    socket.receive(fullSnapshot);
    await accountCommand("account_status");
    expect(manager.snapshot()).toMatchObject({
      status: "online",
      stale: false,
      confirmedHostIds: [],
    });
    expect(attempts).toEqual([first.server_id, first.server_id, second.server_id]);
    socket.disconnect();
    await accountCommand("account_status");
    expect(manager.snapshot()).toMatchObject({ stale: true, confirmedHostIds: [] });
  },
);
