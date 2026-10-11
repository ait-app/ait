/** @vitest-environment jsdom */
import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defaultHostAppearance } from "@/hosts/appearance";
import { createAccountRelayHostConnection, type HostProfile } from "@/types/host-connection";
import { HostRuntimeStore } from "./host-runtime";

const registryKey = "@paseo:daemon-registry";
const center = "https://center.test/api";
const discovered = [
  { host_id: "11111111-1111-4111-8111-111111111111", server_id: "first-server", name: "First" },
  { host_id: "22222222-2222-4222-8222-222222222222", server_id: "second-server", name: "Second" },
];
const direct: HostProfile = {
  serverId: "first-server",
  label: "My workstation",
  appearance: defaultHostAppearance(),
  lifecycle: {},
  connections: [
    { id: "direct:localhost:6767", type: "directTcp", endpoint: "localhost:6767", useTls: false },
  ],
  preferredConnectionId: "direct:localhost:6767",
  createdAt: "2026-10-01T00:00:00.000Z",
  updatedAt: "2026-10-01T00:00:00.000Z",
};
let store: HostRuntimeStore;
let data: Map<string, string>;
let write: ReturnType<typeof vi.fn<(key: string, value: string) => Promise<void>>>;
beforeEach(async () => {
  data = new Map([
    ["@paseo:e2e", "1"],
    [registryKey, JSON.stringify([direct])],
  ]);
  write = vi.fn(async (key, value) => {
    data.set(key, value);
  });
  store = new HostRuntimeStore({
    storage: {
      getItem: async (key) => data.get(key) ?? null,
      setItem: write,
      removeItem: async (key) => {
        data.delete(key);
      },
    },
    revokePushNotifications: vi.fn(async () => {}),
  });
  // Check real registry and persistence behavior without opening network connections.
  vi.spyOn(store, "syncHosts").mockImplementation(() => {});
  await store.boot();
  store.setAccountRelayCenter(center);
  write.mockClear();
});

describe("automatic account host discovery", () => {
  it("persists all hosts in one write and merges the direct host with its preferences intact", async () => {
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts().map((host) => host.serverId)).toEqual([
      "first-server",
      "second-server",
    ]);
    expect(store.getHosts()[0]).toMatchObject({
      ...direct,
      updatedAt: expect.any(String),
      connections: [
        direct.connections[0],
        createAccountRelayHostConnection({ hostId: discovered[0]!.host_id, center }),
      ],
    });
    expect(write).toHaveBeenCalledOnce();
    expect(JSON.parse(data.get(registryKey)!)).toEqual(store.getHosts());
    const hosts = store.getHosts();
    const listener = vi.fn();
    store.subscribeHostList(listener);
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts()).toBe(hosts);
    expect(write).toHaveBeenCalledOnce();
    expect(listener).not.toHaveBeenCalled();
  });

  it("imports a newly discovered host while retaining offline and signed-out records", async () => {
    await store.addDiscoveredAccountRelayHosts([discovered[0]!], center);
    await store.addDiscoveredAccountRelayHosts([discovered[1]!], center);
    expect(store.getHosts()).toHaveLength(2);
    store.setAccountRelayCenter(null);
    expect(store.getHosts()).toHaveLength(2);
    expect(JSON.parse(data.get(registryKey)!)).toHaveLength(2);
  });

  it("does not recreate a deleted host during refresh but imports it on a new login", async () => {
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    await store.removeHost("second-server");
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts().map((host) => host.serverId)).toEqual(["first-server"]);
    store.setAccountRelayCenter(null);
    store.setAccountRelayCenter(center);
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts()).toHaveLength(2);
  });

  it("keeps a deleted relay connection absent while retaining direct access", async () => {
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    const relay = createAccountRelayHostConnection({ hostId: discovered[0]!.host_id, center });
    await store.removeConnection("first-server", relay.id);
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts()[0]?.connections).toEqual(direct.connections);
    expect(store.getHosts()[1]?.connections[0]?.type).toBe("accountRelay");
  });

  it("reports storage failure without marking hosts as imported, allowing a retry", async () => {
    write.mockRejectedValueOnce(new Error("Disk full"));
    await expect(store.addDiscoveredAccountRelayHosts(discovered, center)).rejects.toThrow(
      "Disk full",
    );
    expect(store.getHosts()).toHaveLength(1);
    expect(store.getHosts()[0]?.connections).toEqual(direct.connections);
    await store.addDiscoveredAccountRelayHosts(discovered, center);
    expect(store.getHosts()).toHaveLength(2);
  });

  it("serializes overlapping discoveries so each new host is retained", async () => {
    let finish!: () => void;
    write.mockImplementationOnce(
      (key, value) =>
        new Promise<void>((resolve) => {
          finish = () => {
            data.set(key, value);
            resolve();
          };
        }),
    );
    const first = store.addDiscoveredAccountRelayHosts([discovered[0]!], center);
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    const second = store.addDiscoveredAccountRelayHosts([discovered[1]!], center);
    finish();
    await Promise.all([first, second]);
    expect(store.getHosts()).toHaveLength(2);
    expect(JSON.parse(data.get(registryKey)!)).toHaveLength(2);
  });

  it("does not activate a discovery superseded while storage is pending", async () => {
    let current = true;
    let finish!: () => void;
    write.mockImplementationOnce(
      (key, value) =>
        new Promise<void>((resolve) => {
          finish = () => {
            data.set(key, value);
            resolve();
          };
        }),
    );
    const importing = store.addDiscoveredAccountRelayHosts(discovered, center, () => current);
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    current = false;
    store.setAccountRelayCenter(null);
    finish();
    await importing;
    expect(store.getHosts()).toHaveLength(1);
    expect(store.getHosts()[0]?.connections).toEqual(direct.connections);
  });

  it("retains a local rename made while discovery storage is pending", async () => {
    let finish!: () => void;
    write.mockImplementationOnce(
      (key, value) =>
        new Promise<void>((resolve) => {
          finish = () => {
            data.set(key, value);
            resolve();
          };
        }),
    );
    const importing = store.addDiscoveredAccountRelayHosts(discovered, center);
    await vi.waitFor(() => expect(write).toHaveBeenCalledOnce());
    const renaming = store.renameHost("first-server", "Renamed while importing");
    finish();
    await Promise.all([importing, renaming]);
    expect(store.getHosts()).toHaveLength(2);
    expect(store.getHosts()[0]?.label).toBe("Renamed while importing");
    expect(JSON.parse(data.get(registryKey)!)).toEqual(store.getHosts());
  });

  it("skips a stale discovery before writing to storage", async () => {
    await store.addDiscoveredAccountRelayHosts(discovered, center, () => false);
    expect(write).not.toHaveBeenCalled();
    expect(store.getHosts()).toHaveLength(1);
  });
});
