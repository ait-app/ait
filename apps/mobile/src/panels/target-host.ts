import type { WorkspaceTabTarget } from "@/workspace-tabs/model";
import { panelSupportsHost, type PaneHost } from "./panel-manifest";

export function panelTargetSupportsHost(target: WorkspaceTabTarget, host: PaneHost): boolean {
  return target.kind !== "plugin" && panelSupportsHost(target.kind, host);
}
