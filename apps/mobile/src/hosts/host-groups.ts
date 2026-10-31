import type { AccountRelayHostConnection, HostConnection } from "@/types/host-connection";

export interface HostChoice {
  serverId: string;
  label: string;
  connections?: readonly HostConnection[];
}

export type HostGroup<T extends HostChoice = HostChoice> = T & {
  memberServerIds: string[];
};

function sharedConnection(host: HostChoice): AccountRelayHostConnection | undefined {
  return host.connections?.find(
    (connection): connection is AccountRelayHostConnection =>
      connection.type === "accountRelay" && connection.shareId !== undefined,
  );
}

function scopePriority(host: HostChoice): number {
  switch (sharedConnection(host)?.shareResourceKind) {
    case "node":
      return 0;
    case "project":
      return 1;
    default:
      return 2;
  }
}

/** Presentation only: grants retain separate clients, directories, and authorization. */
export function groupHosts<T extends HostChoice>(hosts: readonly T[]): HostGroup<T>[] {
  const groups = new Map<string, HostGroup<T>>();
  for (const host of hosts) {
    const connection = sharedConnection(host);
    const key = connection
      ? JSON.stringify(["shared", connection.center, connection.hostId])
      : JSON.stringify(["owned", host.serverId]);
    const existing = groups.get(key);
    if (!existing) {
      groups.set(key, { ...host, memberServerIds: [host.serverId] });
      continue;
    }
    existing.memberServerIds.push(host.serverId);
    // A node grant exposes Host management; otherwise prefer a project grant.
    // Break ties by stable runtime identity so snapshot reordering does not switch targets.
    const priority = scopePriority(host) - scopePriority(existing);
    if (priority < 0 || (priority === 0 && host.serverId < existing.serverId)) {
      groups.set(key, { ...host, memberServerIds: existing.memberServerIds });
    }
  }
  return [...groups.values()];
}

/** Resolve a visible Host selection to all of its independent grant runtimes. */
export function getHostGroupServerIds(hosts: readonly HostChoice[], serverId: string): string[] {
  return (
    groupHosts(hosts).find((host) => host.memberServerIds.includes(serverId))?.memberServerIds ?? [
      serverId,
    ]
  );
}

export function resolveHostGroupServerId(hosts: readonly HostChoice[], serverId: string): string {
  return (
    groupHosts(hosts).find((host) => host.memberServerIds.includes(serverId))?.serverId ?? serverId
  );
}

/** Missing scope uses the same restrictive default as the guest transport. */
export function getSharedHostScope(host: HostChoice): "node" | "project" | "workspace" | null {
  const connection = sharedConnection(host);
  return connection ? (connection.shareResourceKind ?? "workspace") : null;
}
