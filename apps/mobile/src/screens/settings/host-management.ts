import type { HubShare } from "@ait/client/internal/hub";
import type { HostProfile } from "@/types/host-connection";

/** Unknown shared grants fail closed until their scope arrives from the account Hub. */
export function canManageHost(host: HostProfile | null, shares: readonly HubShare[]): boolean {
  if (!host) return false;
  const grants = host.connections.filter(
    (connection) => connection.type === "accountRelay" && connection.shareId !== undefined,
  );
  if (!grants.length) return true;
  return grants.every(
    (connection) =>
      connection.type === "accountRelay" &&
      shares.some((share) => share.shareId === connection.shareId && share.resourceKind === "node"),
  );
}

/** Shared runtime IDs name grants; daemon lifecycle checks use the physical server identity. */
export function daemonManagementServerId(
  runtimeId: string,
  physicalId: string | undefined,
): string {
  return runtimeId.startsWith("shared:") ? (physicalId ?? "") : runtimeId;
}
