import { useCallback, useMemo, useSyncExternalStore } from "react";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { AgentPermissionRequest } from "@ait/protocol/agent-types";

/** An absent snapshot permission does not prove that an in-flight response succeeded. */
export function useRetainedPermissions(
  client: DaemonClient | null,
  agentId: string,
  requests: AgentPermissionRequest[],
): AgentPermissionRequest[] {
  const version = useSyncExternalStore(
    useCallback(
      (listener) => client?.permissionResponses?.subscribe(listener) ?? (() => {}),
      [client],
    ),
    () => client?.permissionResponses?.version ?? 0,
  );
  return useMemo(() => {
    const merged = new Map(requests.map((request) => [request.id, request]));
    for (const request of client?.permissionResponses?.unconfirmed(agentId) ?? []) {
      if (!merged.has(request.id)) merged.set(request.id, request);
    }
    return [...merged.values()];
  }, [client, agentId, requests, version]);
}
