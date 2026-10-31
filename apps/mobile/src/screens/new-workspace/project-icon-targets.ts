import type { HostProjectListItem } from "@/projects/host-projects";
import { createProjectIconTarget, type ProjectIconTarget } from "@/projects/icon-target";

export function buildNewWorkspaceProjectIconTargets(
  projects: readonly HostProjectListItem[],
  serverId: string | readonly string[],
): ProjectIconTarget[] {
  const serverIds = typeof serverId === "string" ? [serverId] : serverId;
  return projects.flatMap((project) => {
    const host = serverIds.flatMap((id) =>
      project.hosts.filter((candidate) => candidate.serverId === id),
    )[0];
    if (!host) return [];
    const target = createProjectIconTarget({ projectViewKey: project.viewKey, placement: host });
    return target ? [target] : [];
  });
}
