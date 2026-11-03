import { describe, expect, it, vi } from "vitest";
import type { HubAccountSnapshot, HubShare } from "@ait/client/internal/hub";
import {
  createSharedHostPreferencesStore,
  isSharedHostEnabled,
} from "@/stores/shared-host-preferences-store";
import { bindSharedHosts, groupIncomingSharedHosts } from "./shared-hosts";

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));

const account = { center: "https://center.test/api", accountId: "recipient" };
const share: HubShare = {
  shareId: "project-grant",
  groupId: "team",
  groupName: "Team",
  hostId: "host",
  nodeId: "node",
  hostName: "Workstation",
  owner: { userId: "owner", displayName: "Owner" },
  resourceKind: "project",
  resourceKey: "project",
  displayName: "Project",
  projectName: "Project",
  permission: "operate",
  state: "online",
};
const shares: HubShare[] = [
  share,
  { ...share, shareId: "workspace-grant", resourceKind: "workspace", resourceKey: "workspace" },
  { ...share, shareId: "host-grant", resourceKind: "node", resourceKey: "", groupName: "Friends" },
  { ...share, shareId: "other-grant", hostId: "other-host" },
];

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
}

function createHub() {
  let snapshot: HubAccountSnapshot = {
    state: "online",
    linkId: "link",
    centerUrl: account.center,
    methods: [],
    shares,
  };
  const listeners = new Set<() => void>();
  return {
    snapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    publish: (next: Partial<HubAccountSnapshot>) => {
      snapshot = { ...snapshot, ...next };
      for (const listener of listeners) listener();
    },
  };
}

describe("incoming shared Hosts", () => {
  it("groups grants by Host identity, including identically named Hosts", () => {
    expect(groupIncomingSharedHosts(shares)).toEqual([
      { hostId: "host", label: "Owner · Workstation", groupNames: ["Team", "Friends"] },
      { hostId: "other-host", label: "Owner · Workstation", groupNames: ["Team"] },
    ]);
  });

  it("defaults on and remembers hidden Hosts across reloads, scoped to the recipient and center", async () => {
    const storage = memoryStorage();
    const first = createSharedHostPreferencesStore(storage);
    await first.persist.rehydrate();
    expect(isSharedHostEnabled(first.getState(), account, share.hostId)).toBe(true);
    first.getState().setHostEnabled(account, share.hostId, false);
    const restored = createSharedHostPreferencesStore(storage);
    await restored.persist.rehydrate();

    expect(isSharedHostEnabled(restored.getState(), account, share.hostId)).toBe(false);
    expect(
      isSharedHostEnabled(
        restored.getState(),
        { ...account, center: `${account.center}/` },
        share.hostId,
      ),
    ).toBe(false);
    expect(
      isSharedHostEnabled(
        restored.getState(),
        { ...account, accountId: "someone-else" },
        share.hostId,
      ),
    ).toBe(true);
    expect(
      isSharedHostEnabled(
        restored.getState(),
        { ...account, center: "https://other.test/api" },
        share.hostId,
      ),
    ).toBe(true);
    restored.getState().setHostEnabled(account, share.hostId, true);
    const enabled = createSharedHostPreferencesStore(storage);
    await enabled.persist.rehydrate();
    expect(isSharedHostEnabled(enabled.getState(), account, share.hostId)).toBe(true);
  });

  it("removes every grant for a disabled Host and restores it without losing incoming records", async () => {
    const preferences = createSharedHostPreferencesStore(memoryStorage());
    await preferences.persist.rehydrate();
    const hub = createHub();
    const runtime = { setSharedAccountRelayResources: vi.fn() };
    const stop = bindSharedHosts(hub, runtime, account, preferences);
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith(shares, account.center);

    preferences.getState().setHostEnabled(account, share.hostId, false);
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith(
      [shares[3]],
      account.center,
    );
    expect(hub.snapshot().shares).toBe(shares);

    // Reconnection and fresh grants on the same Host must respect the existing choice.
    hub.publish({ linkId: "new-link", shares: [...shares, { ...share, shareId: "new-grant" }] });
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith(
      [shares[3]],
      account.center,
    );
    preferences.getState().setHostEnabled(account, share.hostId, true);
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith(
      hub.snapshot().shares,
      account.center,
    );

    hub.publish({ shares: [] });
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith([], account.center);
    stop();
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith([], null);
    runtime.setSharedAccountRelayResources.mockClear();
    hub.publish({ shares });
    preferences.getState().setHostEnabled(account, share.hostId, false);
    expect(runtime.setSharedAccountRelayResources).not.toHaveBeenCalled();
  });

  it("does not create hidden shared connections before preferences finish loading", async () => {
    const saved = createSharedHostPreferencesStore(memoryStorage());
    await saved.persist.rehydrate();
    saved.getState().setHostEnabled(account, share.hostId, false);
    let resolveRead!: (value: string) => void;
    const read = new Promise<string>((resolve) => {
      resolveRead = resolve;
    });
    const preferences = createSharedHostPreferencesStore({
      ...memoryStorage(),
      getItem: () => read,
    });
    const runtime = { setSharedAccountRelayResources: vi.fn() };
    const stop = bindSharedHosts(createHub(), runtime, account, preferences);
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith([], account.center);
    resolveRead(
      JSON.stringify({
        state: { disabledHostKeys: saved.getState().disabledHostKeys },
        version: 0,
      }),
    );
    await vi.waitFor(() => expect(preferences.getState().hydrated).toBe(true));
    expect(runtime.setSharedAccountRelayResources).toHaveBeenLastCalledWith(
      [shares[3]],
      account.center,
    );
    stop();
  });
});
