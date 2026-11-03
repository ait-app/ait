// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useState } from "react";
import type { HostProjectListItem } from "@/projects/host-projects";
import { useNewWorkspaceProjectPicker } from "./project-picker";

function project(input: {
  viewKey: string;
  projectKey: string | null;
  projectId: string;
  projectName: string;
}): HostProjectListItem {
  return {
    ...input,
    projectKind: "git",
    iconWorkingDir: `/work/${input.projectId}`,
    hosts: [
      {
        serverId: "host",
        projectId: input.projectId,
        iconWorkingDir: `/work/${input.projectId}`,
        worktreeSupport: "supported",
      },
    ],
    workspaceKeys: [],
  };
}

describe("useNewWorkspaceProjectPicker", () => {
  it("offers projects from every grant on the selected Host and selects their own connection", () => {
    const first = project({
      viewKey: "first",
      projectKey: null,
      projectId: "p1",
      projectName: "First",
    });
    const second = project({
      viewKey: "second",
      projectKey: null,
      projectId: "p2",
      projectName: "Second",
    });
    first.hosts[0].serverId = "share-a";
    second.hosts[0].serverId = "share-b";
    const other = project({
      viewKey: "other",
      projectKey: null,
      projectId: "p3",
      projectName: "Other Host",
    });
    const projects = [first, second, other];
    const projectServerIds = ["share-a", "share-b"];
    const { result } = renderHook(() => {
      const [selectedServerId, onSelectServerId] = useState("share-a");
      const input = {
        selectedServerId,
        projectServerIds,
        onSelectServerId,
        projects,
        routeProject: null,
        routeProjectContextViewKey: null,
        lastActiveProject: null,
        allowAllProjects: true,
      };
      return { ...useNewWorkspaceProjectPicker(input), serverId: selectedServerId };
    });
    expect(result.current.projectPickerOptions.map((option) => option.label)).toEqual([
      "First",
      "Second",
    ]);
    act(() => result.current.handleSelectProjectOption("project:second"));
    expect(result.current.serverId).toBe("share-b");
    expect(result.current.selectedProject).toEqual(second);
    expect(result.current.selectedSourceDirectory).toBe("/work/p2");
    expect(result.current.projectPickerOptions).toHaveLength(2);
    act(() => result.current.handleSelectProjectOption("project:first"));
    expect(result.current.serverId).toBe("share-a");
    expect(result.current.selectedProject).toEqual(first);
  });

  it("preserves a manual choice when the routed project hydrates", () => {
    const routePlacement = project({
      viewKey: '["host","route-local"]',
      projectKey: null,
      projectId: "route-local",
      projectName: "Route project",
    });
    const hydratedRouteProject = project({
      viewKey: "remote:github.com/acme/route",
      projectKey: "remote:github.com/acme/route",
      projectId: "route-local",
      projectName: "Route project",
    });
    const manualProject = project({
      viewKey: "remote:github.com/acme/manual",
      projectKey: "remote:github.com/acme/manual",
      projectId: "manual-local",
      projectName: "Manual project",
    });
    const { result, rerender } = renderHook(
      ({ routeProject, projects }) =>
        useNewWorkspaceProjectPicker({
          selectedServerId: "host",
          projects,
          routeProject,
          routeProjectContextViewKey: routePlacement.viewKey,
          lastActiveProject: null,
          allowAllProjects: true,
        }),
      {
        initialProps: {
          routeProject: routePlacement,
          projects: [routePlacement, manualProject],
        },
      },
    );

    const manualOption = result.current.projectPickerOptions.find(
      (option) => option.label === manualProject.projectName,
    );
    expect(manualOption).toBeDefined();
    act(() => result.current.handleSelectProjectOption(manualOption!.id));
    expect(result.current.selectedProject).toEqual(manualProject);

    rerender({
      routeProject: hydratedRouteProject,
      projects: [hydratedRouteProject, manualProject],
    });

    expect(result.current.selectedProject).toEqual(manualProject);
  });
});
