import type { Href } from "expo-router";
import { buildHostRootRoute, buildHostWorkspaceOpenRoute } from "@/utils/host-routes";
import { trimNonEmpty } from "@/utils/string";

type NotificationData = Record<string, unknown> | null | undefined;
type NotificationRoute = Extract<Href, string>;

export function resolveNotificationTarget(data: NotificationData): {
  serverId: string | null;
  agentId: string | null;
  workspaceId: string | null;
  terminalId: string | null;
} {
  return {
    serverId: trimNonEmpty(data?.serverId),
    agentId: trimNonEmpty(data?.agentId),
    workspaceId: trimNonEmpty(data?.workspaceId),
    terminalId: trimNonEmpty(data?.terminalId),
  };
}

export function buildNotificationRoute(data: NotificationData): NotificationRoute {
  const { serverId, agentId, workspaceId, terminalId } = resolveNotificationTarget(data);
  if (serverId && workspaceId && agentId) {
    return buildHostWorkspaceOpenRoute(serverId, workspaceId, `agent:${agentId}`);
  }
  if (serverId && workspaceId && terminalId) {
    return buildHostWorkspaceOpenRoute(serverId, workspaceId, `terminal:${terminalId}`);
  }
  if (serverId) {
    return buildHostRootRoute(serverId);
  }
  return "/" as const;
}
