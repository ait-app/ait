import { STATUS_BUCKET_ORDER } from "@/utils/sidebar-agent-state";
import type { SidebarWorkspaceEntry } from "@/hooks/sidebar-workspaces-view-model";
import { compareSidebarWorkspaces } from "@/utils/sidebar-workspace-order";

export type StatusBucket = SidebarWorkspaceEntry["statusBucket"];

export { STATUS_BUCKET_ORDER };

export const STATUS_BUCKET_LABELS: Record<StatusBucket, string> = {
  needs_input: "Needs input",
  failed: "Failed",
  attention: "Ready to review",
  running: "Working",
  done: "Done",
};

export interface StatusGroup {
  bucket: StatusBucket;
  label: string;
  rows: SidebarWorkspaceEntry[];
}

export function buildStatusGroups(
  workspaces: SidebarWorkspaceEntry[],
  projectNamesByViewKey: Map<string, string>,
): StatusGroup[] {
  const bucketRows = new Map<StatusBucket, SidebarWorkspaceEntry[]>();

  for (const ws of workspaces) {
    const bucket: StatusBucket = ws.statusBucket;
    let rows = bucketRows.get(bucket);
    if (!rows) {
      rows = [];
      bucketRows.set(bucket, rows);
    }
    rows.push(ws);
  }

  const groups: StatusGroup[] = [];

  for (const bucket of STATUS_BUCKET_ORDER) {
    const rows = bucketRows.get(bucket);
    if (!rows || rows.length === 0) continue;

    rows.sort((a, b) => compareSidebarWorkspaces(a, b, projectNamesByViewKey));
    groups.push({ bucket, label: STATUS_BUCKET_LABELS[bucket], rows });
  }

  return groups;
}

export function buildStatusShortcutIndex(groups: StatusGroup[]): Map<string, number> {
  const index = new Map<string, number>();
  let shortcutNumber = 1;
  for (const group of groups) {
    for (const row of group.rows) {
      if (shortcutNumber > 9) return index;
      index.set(row.workspaceKey, shortcutNumber);
      shortcutNumber += 1;
    }
  }
  return index;
}
