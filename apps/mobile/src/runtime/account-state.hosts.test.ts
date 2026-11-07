/** @vitest-environment jsdom */
import "fake-indexeddb/auto";
import { afterEach, expect, it, vi } from "vitest";
import { AccountSessionManager, type AccountHost } from "@ait/client/internal/account-session";
import type { HubLink } from "@ait/client/internal/hub";
import { centerLink, REGISTRATION_HUB } from "../../../../packages/client/test/hub";
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

/** A `hosts` entry as the center reports it. */
function hubHost(host: AccountHost, state = "online"): object {
  return {
    host_id: host.host_id,
    node_id: host.node_id,
    server_id: host.server_id,
    instance_id: host.instance_id,
    name: host.name,
    platform: host.platform,
    state,
    offline_since: state === "offline" ? "earlier" : null,
    f: [],
  };
}

it("imports Hosts and requests connections from the Hub link's hosts frames", async () => {
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
    status: "active",
    relay_modes: [],
  };
  const second: AccountHost = {
    ...first,
    host_id: "22222222-2222-4222-8222-222222222222",
    node_id: "second-node",
    server_id: "second-server",
    name: "Second",
  };
  const manager: AccountSessionManager = new AccountSessionManager({
    installationId: "client-installation",
    deviceName: "Client",
    platform: "web",
    appVersion: "test",
    randomUUID: () => crypto.randomUUID(),
    save: async () => {},
    // Like the apps: the Hub link runs while signed in.
    notify: (snapshot) => {
      if (snapshot.status === "logged_out") hub.link.stop();
      else hub.link.start();
    },
    closeTransports: () => {},
    fetch: async (input) => {
      if (String(input).endsWith("/nodes/register"))
        return Response.json({
          node_id: "client",
          node_session_id: "client-session",
          host_id: null,
          control_required: false,
          ...REGISTRATION_HUB,
        });
      if (String(input).endsWith("/hub-tickets"))
        return Response.json({
          hub_ticket: "f".repeat(64),
          hub_path: "/v1/hub",
          protocol: "ait-hub-v1",
          hub_features: REGISTRATION_HUB.hub_features,
        });
      return new Response(null, { status: 204 });
    },
  });
  const hub = centerLink(
    () => manager.openHubTicket(),
    () => [],
  );
  hub.center.auto = false;
  // The fixture link comes from the SDK sources; the app types use its build.
  manager.attachHub(hub.link as unknown as HubLink);
  runtime.manager = manager;
  await manager.restore({
    center: "https://center.test/api",
    token: "test-token",
    name: "Me",
    expiresAt: Date.now() + 3600_000,
  });
  await vi.advanceTimersByTimeAsync(1);
  let socket = hub.socket();
  socket.open();
  socket.welcome();
  // A delta before the first full snapshot cannot tell which Hosts exist: it is ignored.
  socket.receive({ t: "hosts", seq: 1, ver: 1 }, JSON.stringify({ upsert: [hubHost(first)] }));
  await accountCommand("account_status");
  expect(manager.snapshot()).toMatchObject({ stale: true, confirmedHostIds: [], hosts: [] });
  expect(store.getHosts()).toHaveLength(0);
  expect(attempts).toEqual([]);

  // The full snapshot imports every Host and requests a connection to each online one.
  socket.receive(
    { t: "hosts", seq: 2, ver: 2, full: true },
    JSON.stringify({ hosts: [hubHost(first), hubHost(second)] }),
  );
  await accountCommand("account_status");
  expect(manager.snapshot()).toMatchObject({
    status: "online",
    stale: false,
    confirmedHostIds: [first.host_id, second.host_id],
  });
  expect(useAccountState.getState().stale).toBe(false);
  expect(store.getHosts().map((host) => host.serverId)).toEqual([
    first.server_id,
    second.server_id,
  ]);
  expect(attempts).toEqual([first.server_id, second.server_id]);

  // Repeated updates do not restart an already available Host.
  socket.receive({ t: "hosts", seq: 3, ver: 3 }, JSON.stringify({ upsert: [hubHost(first)] }));
  await accountCommand("account_status");
  expect(attempts).toEqual([first.server_id, second.server_id]);
  socket.receive(
    { t: "hosts", seq: 4, ver: 4 },
    JSON.stringify({ upsert: [hubHost(first, "offline")] }),
  );
  await accountCommand("account_status");
  expect(manager.snapshot().confirmedHostIds).toEqual([second.host_id]);
  expect(manager.snapshot().hosts.find((host) => host.host_id === first.host_id)).toMatchObject({
    online: false,
  });
  expect(attempts).toEqual([first.server_id, second.server_id]);

  // The network boundary reports established business connections independently of the link.
  const readSnapshot = store.getSnapshot.bind(store);
  vi.spyOn(store, "getSnapshot").mockImplementation((serverId) => {
    const snapshot = readSnapshot(serverId);
    return snapshot ? { ...snapshot, connectionStatus: "online" } : null;
  });
  const available = store.getAvailableHosts();
  expect(available.map((host) => host.serverId)).toEqual([second.server_id]);
  socket.drop();
  await accountCommand("account_status");
  expect(manager.snapshot()).toMatchObject({ stale: true, confirmedHostIds: [] });
  expect(store.getAvailableHosts()).toEqual(available);

  // A fresh link replaces the lost one; its full snapshot is authoritative again.
  await vi.advanceTimersByTimeAsync(4000);
  socket = hub.socket();
  expect(hub.sockets).toHaveLength(2);
  socket.open();
  socket.welcome({ link: "link-2" });
  await accountCommand("account_status");
  expect(manager.snapshot()).toMatchObject({ stale: true, confirmedHostIds: [] });
  const offline = {
    ...first,
    host_id: "33333333-3333-4333-8333-333333333333",
    server_id: "offline-server",
    instance_id: null,
  };
  socket.receive(
    { t: "hosts", seq: 1, ver: 1, full: true },
    JSON.stringify({
      hosts: [
        hubHost(first, "offline"),
        hubHost(second),
        hubHost(offline, "offline"),
        {
          ...hubHost(offline, "offline"),
          host_id: "44444444-4444-4444-8444-444444444444",
          server_id: null,
        },
      ],
    }),
  );
  await accountCommand("account_status");
  expect(manager.snapshot()).toMatchObject({
    status: "online",
    stale: false,
    confirmedHostIds: [second.host_id],
  });
  expect(store.getHosts().map((host) => host.serverId)).toEqual([
    first.server_id,
    second.server_id,
    offline.server_id,
  ]);
  expect(store.getAvailableHosts().map((host) => host.serverId)).toEqual([second.server_id]);
  expect(attempts).not.toContain(offline.server_id);
  expect(JSON.parse(storage.get("@paseo:daemon-registry")!)).toEqual(
    expect.arrayContaining([expect.objectContaining({ serverId: offline.server_id })]),
  );
});
