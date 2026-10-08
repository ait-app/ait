import { describe, expect, test } from "vitest";
import type { ProjectDescriptor, WorkspaceDescriptor } from "@/stores/session-store";
import { buildWorkspaceStructureProjects, createProjectViewKey } from "./workspace-structure";

function project(input: {
  id: string;
  key: string | null;
  root: string;
  name?: string;
}): ProjectDescriptor {
  return {
    projectId: input.id,
    projectKey: input.key,
    projectDisplayName: input.name ?? "acme/app",
    projectCustomName: null,
    projectRootPath: input.root,
    projectKind: "git",
  };
}

function workspace(id: string, projectId: string, root: string): WorkspaceDescriptor {
  return {
    id,
    projectId,
    projectDisplayName: "acme/app",
    projectCustomName: null,
    projectRootPath: root,
    workspaceDirectory: root,
    projectKind: "git",
    workspaceKind: "local_checkout",
    name: "main",
    status: "done",
    statusEnteredAt: null,
    archivingAt: null,
    diffStat: null,
    scripts: [],
  };
}

describe("buildWorkspaceStructureProjects", () => {
  test("scopes identical project and workspace ids to their host", () => {
    const result = buildWorkspaceStructureProjects({
      sessions: ["local", "remote"].map((serverId) => ({
        serverId,
        projects: [
          project({
            id: "prj_ait",
            key: "remote:github.com/acme/ait",
            root: "/repo/ait",
            name: "ait",
          }),
        ],
        workspaces: [workspace("ws-main", "prj_ait", "/repo/ait")],
      })),
    });

    expect(result).toHaveLength(2);
    expect(result.map((item) => item.workspaceKeys)).toEqual([
      ["local:ws-main"],
      ["remote:ws-main"],
    ]);
    expect(new Set(result.map((item) => item.viewKey)).size).toBe(2);
  });

  test("keeps display identity stable when a project's remote key changes", () => {
    const build = (key: string | null) =>
      buildWorkspaceStructureProjects({
        sessions: [
          {
            serverId: "local",
            projects: [project({ id: "prj_ait", key, root: "/repo/ait" })],
            workspaces: [],
          },
        ],
      })[0]!.viewKey;

    expect(build("remote:github.com/acme/ait")).toBe(build(null));
    expect(build("remote:github.com/acme/renamed")).toBe(build(null));
  });

  test("keeps local and remote copies of the same repository in separate project groups", () => {
    const key = "remote:github.com/acme/app";
    const result = buildWorkspaceStructureProjects({
      sessions: [
        {
          serverId: "host-a",
          projects: [project({ id: "prj_a", key, root: "/a/app", name: "ait" })],
          workspaces: [workspace("ws-a", "prj_a", "/a/app")],
        },
        {
          serverId: "host-b",
          projects: [project({ id: "prj_b", key, root: "/b/app", name: "ait" })],
          workspaces: [workspace("ws-b", "prj_b", "/b/app")],
        },
      ],
    });

    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({
      viewKey: createProjectViewKey({ kind: "placement", serverId: "host-a", projectId: "prj_a" }),
      projectKey: key,
      hosts: [{ serverId: "host-a", projectId: "prj_a" }],
      workspaceKeys: ["host-a:ws-a"],
    });
    expect(result[1]).toMatchObject({
      viewKey: createProjectViewKey({ kind: "placement", serverId: "host-b", projectId: "prj_b" }),
      projectKey: key,
      hosts: [{ serverId: "host-b", projectId: "prj_b" }],
      workspaceKeys: ["host-b:ws-b"],
    });
  });

  test("keeps two clones with the same key on one host separate", () => {
    const key = "remote:github.com/acme/app";
    const result = buildWorkspaceStructureProjects({
      sessions: [
        {
          serverId: "host-a",
          projects: [
            project({ id: "prj_one", key, root: "/repos/one" }),
            project({ id: "prj_two", key, root: "/repos/two" }),
          ],
          workspaces: [
            workspace("ws-one", "prj_one", "/repos/one"),
            workspace("ws-two", "prj_two", "/repos/two"),
          ],
        },
      ],
    });

    expect(result).toHaveLength(2);
    expect(result.map((item) => item.projectKey)).toEqual([key, key]);
    expect(result.map((item) => item.hosts[0]?.projectId).sort()).toEqual(["prj_one", "prj_two"]);
    expect(result.map((item) => item.workspaceKeys[0]).sort()).toEqual([
      "host-a:ws-one",
      "host-a:ws-two",
    ]);
  });

  test("does not let project order choose which same-host clone groups with another host", () => {
    const key = "remote:github.com/acme/app";
    const hostAProjects = [
      project({ id: "prj_one", key, root: "/repos/one" }),
      project({ id: "prj_two", key, root: "/repos/two" }),
    ];
    const build = (projects: ProjectDescriptor[]) =>
      buildWorkspaceStructureProjects({
        sessions: [
          { serverId: "host-a", projects, workspaces: [] },
          {
            serverId: "host-b",
            projects: [project({ id: "prj_remote", key, root: "/repos/remote" })],
            workspaces: [],
          },
        ],
      });
    const identitiesByProjectId = (projects: ReturnType<typeof build>) => {
      const identities: Record<string, string> = {};
      for (const group of projects) {
        for (const host of group.hosts) identities[host.projectId] = group.viewKey;
      }
      return identities;
    };

    const forward = build(hostAProjects);
    const reversed = build(hostAProjects.toReversed());

    expect(forward).toHaveLength(3);
    expect(identitiesByProjectId(forward)).toEqual(identitiesByProjectId(reversed));
    expect(new Set(Object.values(identitiesByProjectId(forward))).size).toBe(3);
  });

  test("keeps projects without persisted keys scoped to their host", () => {
    const result = buildWorkspaceStructureProjects({
      sessions: [
        {
          serverId: "host-a",
          projects: [project({ id: "/workspace/app", key: null, root: "/workspace/app" })],
          workspaces: [],
        },
        {
          serverId: "host-b",
          projects: [project({ id: "/workspace/app", key: null, root: "/workspace/app" })],
          workspaces: [],
        },
      ],
    });

    expect(result).toHaveLength(2);
    expect(result.map((item) => item.projectKey)).toEqual([null, null]);
    expect(new Set(result.map((item) => item.viewKey)).size).toBe(2);
  });

  test("does not use opaque project keys as placement view keys", () => {
    const placementShapedKey = createProjectViewKey({
      kind: "placement",
      serverId: "host-a",
      projectId: "prj_b",
    });
    const result = buildWorkspaceStructureProjects({
      sessions: [
        {
          serverId: "host-a",
          projects: [
            project({ id: "prj_a", key: placementShapedKey, root: "/repos/a" }),
            project({ id: "prj_b", key: null, root: "/repos/b" }),
          ],
          workspaces: [],
        },
      ],
    });

    expect(result).toHaveLength(2);
    expect(new Set(result.map((item) => item.viewKey)).size).toBe(2);
    expect(result.find((item) => item.projectKey === placementShapedKey)?.viewKey).toBe(
      createProjectViewKey({ kind: "placement", serverId: "host-a", projectId: "prj_a" }),
    );
  });
});
