import type { SidebarStateBucket } from "@/utils/sidebar-agent-state";

export function shouldRenderSyncedStatusLoader(input: {
  bucket: SidebarStateBucket | null | undefined;
}): boolean {
  return input.bucket === "running";
}
