/**
 * @vitest-environment jsdom
 */
import { act, cleanup, renderHook } from "@testing-library/react";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { HubShare } from "@ait/client/internal/hub";
import type { WorkspaceScriptPayload } from "@ait/protocol/messages";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRoot, type Root } from "react-dom/client";
import React from "react";
import type { ReactElement } from "react";
import { createProjectViewKey } from "@/projects/workspace-structure";

vi.hoisted(() => {
  (globalThis as unknown as { __DEV__: boolean }).__DEV__ = false;
});

const pathnameState = vi.hoisted(() => ({
  value: "/",
}));

vi.mock("expo-router", () => ({
  router: {
    dismissTo: vi.fn(),
  },
  useLocalSearchParams: () => ({}),
  usePathname: () => pathnameState.value,
}));

import {
  createSidebarWorkspaceEntry,
  type SidebarProjectEntry,
} from "@/hooks/use-sidebar-workspaces-list";
import { useSidebarWorkspacesList } from "@/hooks/use-sidebar-workspaces-list";
import {
  getHostRuntimeStore,
  sharedResourceServerId,
  useHosts,
  type HostRuntimeController,
  type HostRuntimeSnapshot,
} from "@/runtime/host-runtime";
import type { HostProfile } from "@/types/host-connection";
import { useSessionStore, type WorkspaceDescriptor } from "@/stores/session-store";
import { seedRuntimeWorkspaces } from "@/test/seed-session";
import { useWorkspaceFields } from "@/stores/session-store-hooks";
import { useActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";
import { defaultHostAppearance } from "@/hosts/appearance";
import { bindSharedHosts } from "@/runtime/shared-hosts";
import { createSharedHostPreferencesStore } from "@/stores/shared-host-preferences-store";

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: {
    getItem: vi.fn().mockResolvedValue(null),
    setItem: vi.fn().mockResolvedValue(undefined),
    removeItem: vi.fn().mockResolvedValue(undefined),
  },
}));

const SERVER_ID = "sidebar-render-count";

interface RenderCounts {
  frame: number;
  headers: Record<string, number>;
  rows: Record<string, number>;
  projectSelection: Record<string, number>;
  rowSelection: Record<string, number>;
}

const runningScript: WorkspaceScriptPayload = {
  scriptName: "web",
  type: "service",
  hostname: "web.paseo.localhost",
  port: 3000,
  lifecycle: "running",
  health: "healthy",
  exitCode: null,
  terminalId: null,
};

function workspace(input: {
  id: string;
  projectId: string;
  projectDisplayName: string;
  name: string;
  status?: WorkspaceDescriptor["status"];
  scripts?: WorkspaceDescriptor["scripts"];
}): WorkspaceDescriptor {
  return {
    id: input.id,
    projectId: input.projectId,
    projectDisplayName: input.projectDisplayName,
    projectRootPath: `/repo/${input.projectId}`,
    workspaceDirectory: `/repo/${input.projectId}/${input.id}`,
    projectKind: "git",
    workspaceKind: input.name === "main" ? "local_checkout" : "worktree",
    name: input.name,
    status: input.status ?? "done",
    statusEnteredAt: null,
    archivingAt: null,
    diffStat: null,
    scripts: input.scripts ?? [],
  };
}

function createWorkspaces(): WorkspaceDescriptor[] {
  return [
    workspace({
      id: "a-main",
      projectId: "project-a",
      projectDisplayName: "Project A",
      name: "main",
      scripts: [runningScript],
    }),
    workspace({
      id: "a-one",
      projectId: "project-a",
      projectDisplayName: "Project A",
      name: "one",
    }),
    workspace({
      id: "a-two",
      projectId: "project-a",
      projectDisplayName: "Project A",
      name: "two",
    }),
    workspace({
      id: "b-main",
      projectId: "project-b",
      projectDisplayName: "Project B",
      name: "main",
    }),
    workspace({
      id: "b-one",
      projectId: "project-b",
      projectDisplayName: "Project B",
      name: "one",
    }),
    workspace({
      id: "b-two",
      projectId: "project-b",
      projectDisplayName: "Project B",
      name: "two",
    }),
  ];
}

function makeHost(): HostProfile {
  const now = "2026-04-19T00:00:00.000Z";
  return {
    serverId: SERVER_ID,
    label: "Render Count Host",
    appearance: defaultHostAppearance(),
    lifecycle: {},
    connections: [],
    preferredConnectionId: null,
    createdAt: now,
    updatedAt: now,
  };
}

function setHostProfiles(hosts: HostProfile[]): void {
  (
    getHostRuntimeStore() as unknown as {
      setHostsAndSync: (hosts: HostProfile[]) => void;
    }
  ).setHostsAndSync(hosts);
}

function initializeSidebarState(workspaces: WorkspaceDescriptor[]): void {
  act(() => {
    setHostProfiles([makeHost()]);
    useSessionStore.getState().initializeSession(SERVER_ID, null as unknown as DaemonClient);
    seedRuntimeWorkspaces(SERVER_ID, new Map(workspaces.map((entry) => [entry.id, entry])));
    useSessionStore.getState().setHasHydratedWorkspaces(SERVER_ID, true);
  });
}

function resetCounts(counts: RenderCounts): void {
  counts.frame = 0;
  counts.headers = {};
  counts.rows = {};
  counts.projectSelection = {};
  counts.rowSelection = {};
}

function incrementRecord(record: Record<string, number>, key: string): void {
  record[key] = (record[key] ?? 0) + 1;
}

function ProjectHeaderProbe({
  project,
  counts,
}: {
  project: SidebarProjectEntry;
  counts: RenderCounts;
}): null {
  incrementRecord(counts.headers, project.viewKey);
  return null;
}

function WorkspaceRowProbe({
  serverId,
  workspaceId,
  counts,
}: {
  serverId: string;
  workspaceId: string;
  counts: RenderCounts;
}): null {
  const workspaceEntry = useWorkspaceFields(serverId, workspaceId, (entry) =>
    createSidebarWorkspaceEntry({ serverId, workspace: entry }),
  );
  if (workspaceEntry) {
    incrementRecord(counts.rows, workspaceEntry.workspaceId);
  }
  return null;
}

function ProjectActiveProbe({
  serverId,
  project,
  counts,
}: {
  serverId: string;
  project: SidebarProjectEntry;
  counts: RenderCounts;
}): null {
  const activeSelection = useActiveWorkspaceSelection();
  const isActive =
    activeSelection?.serverId === serverId &&
    project.workspaces.some((entry) => entry.workspaceId === activeSelection.workspaceId);
  void isActive;
  incrementRecord(counts.projectSelection, project.viewKey);
  return null;
}

function WorkspaceSelectionProbe({
  serverId,
  workspaceId,
  counts,
}: {
  serverId: string;
  workspaceId: string;
  counts: RenderCounts;
}): null {
  const activeSelection = useActiveWorkspaceSelection();
  const selected =
    activeSelection?.serverId === serverId && activeSelection.workspaceId === workspaceId;
  void selected;
  incrementRecord(counts.rowSelection, workspaceId);
  return null;
}

function SidebarFrameProbe({ counts }: { counts: RenderCounts }): ReactElement {
  counts.frame += 1;
  const { projects } = useSidebarWorkspacesList({ hostFilters: [SERVER_ID] });

  return (
    <>
      {projects.map((project) => (
        <div key={project.viewKey}>
          <ProjectHeaderProbe project={project} counts={counts} />
          <ProjectActiveProbe serverId={SERVER_ID} project={project} counts={counts} />
          {project.workspaces.map((entry) => (
            <React.Fragment key={entry.workspaceKey}>
              <WorkspaceRowProbe
                serverId={entry.serverId}
                workspaceId={entry.workspaceId}
                counts={counts}
              />
              <WorkspaceSelectionProbe
                serverId={entry.serverId}
                workspaceId={entry.workspaceId}
                counts={counts}
              />
            </React.Fragment>
          ))}
        </div>
      ))}
    </>
  );
}

function getHostController(): HostRuntimeController {
  const controllers = (
    getHostRuntimeStore() as unknown as {
      controllers: Map<string, HostRuntimeController>;
    }
  ).controllers;
  const controller = controllers.get(SERVER_ID);
  if (!controller) {
    throw new Error("Host runtime controller was not initialized");
  }
  return controller;
}

function updateControllerSnapshot(
  patch: Partial<Omit<HostRuntimeSnapshot, "serverId" | "clientGeneration">>,
): void {
  (
    getHostController() as unknown as {
      updateSnapshot: (
        patch: Partial<Omit<HostRuntimeSnapshot, "serverId" | "clientGeneration">>,
      ) => void;
    }
  ).updateSnapshot(patch);
}

async function renderProbe(counts: RenderCounts): Promise<{ root: Root; container: HTMLElement }> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    renderSidebarFrame(root, counts);
  });
  resetCounts(counts);
  return { root, container };
}

function renderSidebarFrame(root: Root, counts: RenderCounts) {
  root.render(<SidebarFrameProbe counts={counts} />);
}

describe("sidebar workspace render isolation", () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  beforeEach(async () => {
    initializeSidebarState(createWorkspaces());
  });

  afterEach(() => {
    if (root) {
      act(() => {
        root?.unmount();
      });
    }
    root = null;
    container?.remove();
    container = null;
    act(() => {
      pathnameState.value = "/";
      setHostProfiles([]);
      useSessionStore.getState().clearSession(SERVER_ID);
    });
  });

  it("re-renders only the changed workspace row for a status update", async () => {
    const counts: RenderCounts = {
      frame: 0,
      headers: {},
      rows: {},
      projectSelection: {},
      rowSelection: {},
    };
    ({ root, container } = await renderProbe(counts));

    act(() => {
      useSessionStore.getState().mergeWorkspaces(SERVER_ID, [
        {
          ...createWorkspaces()[1],
          status: "running",
        },
      ]);
    });

    expect(counts.frame).toBe(0);
    expect(counts.headers).toEqual({});
    expect(counts.rows).toEqual({ "a-one": 1 });
  });

  it("does not re-render the sidebar for a host-runtime probe tick with no content change", async () => {
    const counts: RenderCounts = {
      frame: 0,
      headers: {},
      rows: {},
      projectSelection: {},
      rowSelection: {},
    };
    ({ root, container } = await renderProbe(counts));

    act(() => {
      const probeByConnectionId = getHostController().getSnapshot().probeByConnectionId;
      updateControllerSnapshot({
        probeByConnectionId: new Map(probeByConnectionId),
      });
    });

    expect(counts).toEqual({
      frame: 0,
      headers: {},
      rows: {},
      projectSelection: {},
      rowSelection: {},
    });
  });

  it("updates active selection probes from the active workspace route", async () => {
    const counts: RenderCounts = {
      frame: 0,
      headers: {},
      rows: {},
      projectSelection: {},
      rowSelection: {},
    };

    act(() => {
      pathnameState.value = `/h/${SERVER_ID}/workspace/a-one`;
    });
    ({ root, container } = await renderProbe(counts));

    act(() => {
      pathnameState.value = `/h/${SERVER_ID}/workspace/b-two`;
      if (root) {
        renderSidebarFrame(root, counts);
      }
    });

    expect(counts.frame).toBe(1);
    expect(counts.projectSelection).toEqual({
      [createProjectViewKey({ kind: "equivalence", projectKey: "project-a" })]: 1,
      [createProjectViewKey({ kind: "equivalence", projectKey: "project-b" })]: 1,
    });
    expect(counts.rowSelection).toEqual({
      "a-main": 1,
      "a-one": 1,
      "a-two": 1,
      "b-main": 1,
      "b-one": 1,
      "b-two": 1,
    });
  });
});

describe("shared resources in the regular sidebar lists", () => {
  afterEach(() => {
    cleanup();
    setHostProfiles([]);
    getHostRuntimeStore().setAccountRelayCenter(null);
    useSessionStore.getState().clearSession(SERVER_ID);
  });

  it.each(["node", "project", "workspace"] as const)(
    "includes an incoming %s share without opening the sharing screen",
    async (resourceKind) => {
      initializeSidebarState(createWorkspaces());
      const preferences = createSharedHostPreferencesStore({
        getItem: async () => null,
        setItem: async () => {},
        removeItem: async () => {},
      });
      await preferences.persist.rehydrate();
      const account = { center: "https://center.test/api", accountId: "recipient" };
      let stopSharedHosts = () => {};
      const share: HubShare = {
        shareId: "22222222-2222-4222-8222-222222222222",
        groupId: "44444444-4444-4444-8444-444444444444",
        groupName: "Team",
        hostId: "11111111-1111-4111-8111-111111111111",
        nodeId: "shared-node",
        hostName: "Alice workstation",
        owner: { userId: "alice", displayName: "Alice", username: "alice" },
        resourceKind,
        resourceKey:
          resourceKind === "node"
            ? ""
            : resourceKind === "project"
              ? "shared-project"
              : "shared-workspace",
        displayName: "Shared resource",
        projectName: "Shared project",
        permission: "operate",
        state: "online",
      };
      const serverId = sharedResourceServerId(share.shareId);
      const runtime = getHostRuntimeStore();
      const acquireDirectoryDemand = vi.spyOn(runtime, "acquireDirectoryDemand");
      const initialProps: { hostFilters: string[] } = { hostFilters: [] };
      const { result, rerender } = renderHook(
        ({ hostFilters }: { hostFilters: string[] }) => ({
          hosts: useHosts({ availableOnly: true }),
          list: useSidebarWorkspacesList({ hostFilters }),
        }),
        { initialProps },
      );

      try {
        expect(result.current.list.projects.map((project) => project.projectName)).toEqual([
          "Project A",
          "Project B",
        ]);

        act(() => {
          runtime.setAccountRelayCenter("https://center.test/api");
          stopSharedHosts = bindSharedHosts(
            {
              snapshot: () => ({
                state: "online",
                linkId: "link",
                centerUrl: account.center,
                methods: [],
                shares: [share],
              }),
              subscribe: () => () => {},
            },
            runtime,
            account,
            preferences,
          );
          const sharedWorkspace = workspace({
            id: "shared-workspace",
            projectId: "shared-project",
            projectDisplayName: "Shared project",
            name: "Shared workspace",
          });
          useSessionStore.getState().initializeSession(serverId, null);
          seedRuntimeWorkspaces(serverId, new Map([[sharedWorkspace.id, sharedWorkspace]]));
          useSessionStore.getState().setHasHydratedWorkspaces(serverId, true);
        });

        expect(result.current.hosts.map((host) => host.serverId)).toContain(serverId);
        expect(acquireDirectoryDemand).toHaveBeenCalledWith(serverId);
        expect(result.current.list.projects.map((project) => project.projectName)).toEqual([
          "Project A",
          "Project B",
          "Shared project",
        ]);
        expect(result.current.list.workspacePlacements).toContainEqual(
          expect.objectContaining({
            serverId,
            workspaceId: "shared-workspace",
            workspaceKey: `${serverId}:shared-workspace`,
          }),
        );

        rerender({ hostFilters: [serverId] });
        expect(result.current.list.projects.map((project) => project.projectName)).toEqual([
          "Shared project",
        ]);
        expect(result.current.list.workspacePlacements).toHaveLength(1);

        // One physical Host filter must include its other grant runtimes too.
        const siblingShare: HubShare = {
          ...share,
          shareId: "55555555-5555-4555-8555-555555555555",
          resourceKind: "project",
          resourceKey: "sibling-project",
        };
        const siblingServerId = sharedResourceServerId(siblingShare.shareId);
        act(() => {
          runtime.setSharedAccountRelayResources([share, siblingShare], account.center);
          const siblingWorkspace = workspace({
            id: "sibling-workspace",
            projectId: "sibling-project",
            projectDisplayName: "Sibling project",
            name: "Sibling workspace",
          });
          useSessionStore.getState().initializeSession(siblingServerId, null);
          seedRuntimeWorkspaces(
            siblingServerId,
            new Map([[siblingWorkspace.id, siblingWorkspace]]),
          );
          useSessionStore.getState().setHasHydratedWorkspaces(siblingServerId, true);
        });
        expect(
          result.current.list.workspacePlacements.map((entry) => entry.serverId).sort(),
        ).toEqual([serverId, siblingServerId].sort());

        rerender({ hostFilters: [] });
        act(() => {
          preferences.getState().setHostEnabled(account, share.hostId, false);
        });
        expect(result.current.hosts.map((host) => host.serverId)).not.toContain(serverId);
        expect(result.current.list.projects.map((project) => project.projectName)).toEqual([
          "Project A",
          "Project B",
        ]);
        expect(
          result.current.list.workspacePlacements.some((entry) => entry.serverId === serverId),
        ).toBe(false);

        act(() => {
          preferences.getState().setHostEnabled(account, share.hostId, true);
          const restored = workspace({
            id: "shared-workspace",
            projectId: "shared-project",
            projectDisplayName: "Shared project",
            name: "Shared workspace",
          });
          useSessionStore.getState().initializeSession(serverId, null);
          seedRuntimeWorkspaces(serverId, new Map([[restored.id, restored]]));
          useSessionStore.getState().setHasHydratedWorkspaces(serverId, true);
        });
        expect(result.current.hosts.map((host) => host.serverId)).toContain(serverId);
        expect(result.current.list.projects.map((project) => project.projectName)).toContain(
          "Shared project",
        );
        expect(
          result.current.list.workspacePlacements.some((entry) => entry.serverId === serverId),
        ).toBe(true);
      } finally {
        act(stopSharedHosts);
        acquireDirectoryDemand.mockRestore();
        useSessionStore.getState().clearSession(serverId);
      }
    },
  );
});
