import { describe, expect, it } from "vitest";
import type { HostProjectListItem } from "@/projects/host-projects";
import { buildNewWorkspaceProjectIconTargets } from "./project-icon-targets";

const project: HostProjectListItem = {
  viewKey: "project-view",
  projectKey: "remote:github.com/acme/app",
  projectName: "app",
  projectKind: "git",
  iconWorkingDir: "/work/app",
  hosts: [
    {
      serverId: "host-a",
      projectId: "project-a",
      iconWorkingDir: "/work/app",
      worktreeSupport: "supported",
      customIconRevision: "custom-revision",
      iconRevision: "effective-revision",
    },
  ],
  workspaceKeys: [],
};

describe("buildNewWorkspaceProjectIconTargets", () => {
  it("loads icons through each project's grant within the selected Host", () => {
    const second = {
      ...project,
      viewKey: "second",
      hosts: project.hosts.map((host) => ({
        ...host,
        serverId: "share-b",
        projectId: "project-b",
      })),
    };
    expect(
      buildNewWorkspaceProjectIconTargets([project, second], ["host-a", "share-b"]).map(
        (target) => [target.serverId, target.projectId],
      ),
    ).toEqual([
      ["host-a", "project-a"],
      ["share-b", "project-b"],
    ]);
  });
  it("uses the host effective revision for the sidebar's project-icon cache identity", () => {
    expect(buildNewWorkspaceProjectIconTargets([project], "host-a")).toEqual([
      {
        projectViewKey: "project-view",
        projectId: "project-a",
        serverId: "host-a",
        iconWorkingDir: "/work/app",
        customIconRevision: "custom-revision",
        iconRevision: "effective-revision",
      },
    ]);
  });
});
