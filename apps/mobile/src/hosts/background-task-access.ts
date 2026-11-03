import { getSharedHostScope, type HostChoice } from "./host-groups";

/** Background tasks outlive a guest connection, so only the Host owner may mutate them. */
export function canManageBackgroundTasks(host: HostChoice | string): boolean {
  const serverId = typeof host === "string" ? host : host.serverId;
  return (
    !serverId.startsWith("shared:") &&
    (typeof host === "string" || getSharedHostScope(host) === null)
  );
}
