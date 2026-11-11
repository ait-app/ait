import { useState } from "react";
import { HostMonitorView } from "./view";
import { useHostStats, useMonitorSelection } from "./queries";

export function HostMonitorSidebar({ active }: { active: boolean }) {
  const [open, setOpen] = useState(false);
  const selection = useMonitorSelection();
  const serverId = selection.host?.serverId ?? "";
  const summary = useHostStats(serverId, false, active && !open);
  const detail = useHostStats(serverId, true, active && open);
  const query = open ? detail : summary;
  if (!selection.host) return null;
  return (
    <HostMonitorView
      host={selection.host}
      hosts={selection.hosts}
      picked={selection.picked}
      onSelect={selection.select}
      open={open && active}
      onOpenChange={setOpen}
      connected={query.connected}
      supported={query.supported}
      failed={query.isError}
      snapshot={query.data ?? summary.data}
      onRetry={() => {
        void query.refetch();
      }}
    />
  );
}
