import type { HubAccountConnection, HubShare } from "@ait/client/internal/hub";
import {
  isSharedHostEnabled,
  useSharedHostPreferences,
  type SharedHostAccount,
} from "@/stores/shared-host-preferences-store";
import type { HostRuntimeStore } from "./host-runtime";

interface IncomingSharedHost {
  hostId: string;
  label: string;
  groupNames: string[];
}

/** Several grants from one Host share a single local visibility preference. */
export function groupIncomingSharedHosts(shares: readonly HubShare[]): IncomingSharedHost[] {
  const hosts = new Map<string, IncomingSharedHost>();
  for (const share of shares) {
    const existing = hosts.get(share.hostId);
    if (existing) {
      if (!existing.groupNames.includes(share.groupName)) existing.groupNames.push(share.groupName);
      continue;
    }
    const owner =
      share.owner.displayName?.trim() ||
      (share.owner.username?.trim() ? `@${share.owner.username.trim()}` : "");
    hosts.set(share.hostId, {
      hostId: share.hostId,
      label: owner ? `${owner} · ${share.hostName}` : share.hostName,
      groupNames: [share.groupName],
    });
  }
  return [...hosts.values()];
}

/** Keep the raw Hub snapshot intact so hidden Hosts can still be enabled again. */
export function bindSharedHosts(
  hub: Pick<HubAccountConnection, "snapshot" | "subscribe">,
  runtime: Pick<HostRuntimeStore, "setSharedAccountRelayResources">,
  account: SharedHostAccount,
  preferences = useSharedHostPreferences,
): () => void {
  const sync = () => {
    const snapshot = hub.snapshot();
    const state = preferences.getState();
    // Wait for persisted choices before creating any shared connections.
    const shares = state.hydrated
      ? (snapshot.shares ?? []).filter((share) => isSharedHostEnabled(state, account, share.hostId))
      : [];
    runtime.setSharedAccountRelayResources(shares, snapshot.centerUrl);
  };
  const stopHub = hub.subscribe(sync);
  const stopPreferences = preferences.subscribe(sync);
  sync();
  return () => {
    stopHub();
    stopPreferences();
    runtime.setSharedAccountRelayResources([], null);
  };
}
