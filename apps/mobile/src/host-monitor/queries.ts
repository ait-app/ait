import { useState } from "react";
import { CancelledError, skipToken } from "@tanstack/react-query";
import { useFetchQuery } from "@/data/query";
import { useAppVisible } from "@/hooks/use-app-visible";
import { getSharedHostScope } from "@/hosts/host-groups";
import { useHostFeature } from "@/runtime/host-features";
import { getHostRuntimeStore, useHosts, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import {
  useActiveWorkspaceSelection,
  useLastWorkspaceSelection,
} from "@/stores/navigation-active-workspace-store";
import { resolveMonitorHost } from "./model";

export function useMonitorSelection() {
  const hosts = useHosts();
  const active = useActiveWorkspaceSelection();
  const last = useLastWorkspaceSelection();
  const [picked, select] = useState<string | null>(null);
  const host = resolveMonitorHost(hosts, picked, active?.serverId ?? last?.serverId ?? null);
  return { hosts, host, picked: hosts.some((h) => h.serverId === picked) ? picked : null, select };
}

export function useHostStats(serverId: string, includeHistory: boolean, active: boolean) {
  const visible = useAppVisible();
  const connected = useHostRuntimeIsConnected(serverId);
  const host = useHosts().find((candidate) => candidate.serverId === serverId);
  const scope = host ? getSharedHostScope(host) : null;
  const restricted = scope === "project" || scope === "workspace";
  const supported = useHostFeature(serverId, "hostStats");
  const query = useFetchQuery({
    queryKey: ["host-stats", serverId, includeHistory],
    queryFn: serverId
      ? async ({ signal }) => {
          const client = getHostRuntimeStore().getClient(serverId);
          if (!client) throw new Error("Host connection unavailable");
          const result = await client.getHostStats(includeHistory);
          if (signal.aborted) throw new CancelledError();
          return result;
        }
      : skipToken,
    enabled: active && visible && connected && supported && !restricted,
    dataShape: "value",
    staleTimeMs: 1_000,
    refetchInterval: 2_000,
    refetchIntervalInBackground: false,
    retry: false,
    gcTime: 180_000,
  });
  return { ...query, connected, supported, restricted };
}
