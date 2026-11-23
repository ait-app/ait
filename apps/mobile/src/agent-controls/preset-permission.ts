import { useHosts } from "@/runtime/host-runtime";
import type { HostChoice } from "@/hosts/host-groups";

/** A shared runtime may use presets only when its own grant covers the whole node. */
export function isPresetPermissionDenied(
  serverId: string | null,
  hosts: readonly HostChoice[],
): boolean {
  if (!serverId?.startsWith("shared:")) return false;
  const host = hosts.find((candidate) => candidate.serverId === serverId);
  return !host?.connections?.some(
    (connection) =>
      connection.type === "accountRelay" &&
      connection.shareId !== undefined &&
      `shared:${connection.shareId}` === serverId &&
      connection.shareResourceKind === "node",
  );
}

export function usePresetPermissionDenied(serverId: string | null): boolean {
  return isPresetPermissionDenied(serverId, useHosts());
}
