import { useMemo, useRef } from "react";
import { useQueries } from "@tanstack/react-query";
import type { ProjectIcon } from "@ait/protocol/messages";
import { useHostFeatureAvailabilityMap } from "@/runtime/host-features";
import { projectIconCache } from "@/projects/icon-cache";
import type { ProjectIconTarget } from "@/projects/icon-target";
import { getHostRuntimeStore, isHostRuntimeConnected } from "@/runtime/host-runtime";

function iconDataUri(icon: ProjectIcon | null): string | null {
  if (!icon) return null;
  return `data:${icon.mimeType};base64,${icon.data}`;
}

function useStableIconData(data: (string | null)[], signature: string): readonly (string | null)[] {
  const stableRef = useRef<{ signature: string; data: (string | null)[] } | null>(null);
  if (stableRef.current?.signature !== signature) {
    stableRef.current = { signature, data };
  }
  return stableRef.current.data;
}

export function useProjectIcons(input: {
  projects: readonly ProjectIconTarget[];
}): Map<string, string | null> {
  const serverIds = useMemo(
    () => [...new Set(input.projects.map((project) => project.serverId))],
    [input.projects],
  );
  const supportsCustomIcons = useHostFeatureAvailabilityMap(serverIds, "projectCustomIcon");
  const requests = useMemo(() => {
    const unique = new Map<string, ProjectIconTarget>();
    for (const project of input.projects) {
      if (!project.serverId || !project.projectId || !project.iconWorkingDir.trim()) continue;
      unique.set(`${project.serverId}:${project.projectId}`, project);
    }
    return Array.from(unique.values());
  }, [input.projects]);

  const queries = useQueries({
    queries: requests.map((request) => {
      return {
        ...projectIconCache.query(
          request,
          supportsCustomIcons.get(request.serverId) ?? null,
          () => getHostRuntimeStore().getClient(request.serverId),
          isHostRuntimeConnected(getHostRuntimeStore().getSnapshot(request.serverId)),
        ),
        select: iconDataUri,
      };
    }),
  });

  const signature = queries.map((query) => query.data ?? "").join("\u0000");
  const data = useStableIconData(
    queries.map((query) => query.data ?? null),
    signature,
  );

  return useMemo(() => {
    const byTarget = new Map<string, string | null>();
    requests.forEach((request, index) => {
      byTarget.set(`${request.serverId}:${request.projectId}`, data[index] ?? null);
    });

    const byProject = new Map<string, string | null>();
    for (const project of input.projects) {
      byProject.set(
        project.projectViewKey,
        byTarget.get(`${project.serverId}:${project.projectId}`) ?? null,
      );
    }
    return byProject;
  }, [data, input.projects, requests]);
}
