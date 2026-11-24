import { useCallback, useState } from "react";
import { getHostRuntimeStore } from "@/runtime/host-runtime";
import type { HostProfile } from "@/types/host-connection";

export function useSidebarHostPicker(onOpenHostSettings: (serverId: string) => void) {
  const [hosts, setHosts] = useState<HostProfile[] | null>(null);
  const handleOpenChange = useCallback((open: boolean) => {
    setHosts(open ? getHostRuntimeStore().getAvailableHosts() : null);
  }, []);
  const openHostSettings = useCallback(
    (serverId: string) => {
      // Saved hosts have settings even while offline. A removed host has no settings target.
      if (
        getHostRuntimeStore()
          .getHosts()
          .some((host) => host.serverId === serverId)
      ) {
        onOpenHostSettings(serverId);
      }
    },
    [onOpenHostSettings],
  );
  return { hosts, handleOpenChange, openHostSettings };
}
