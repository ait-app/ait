import { describe, expect, it } from "vitest";
import {
  cloneGithubProjectDirectly,
  getOpenProjectFailureReason,
  openProjectDirectly,
  type GithubCloneProgressListener,
} from "@/hooks/open-project";
import type { ProjectGithubCloneProgressMessage } from "@ait/protocol/messages";
import type { ProjectDescriptor } from "@/stores/session-store";

const SERVER_ID = "server-1";
const PROJECT_PATH = "/repo/project";

function buildProjectPayload() {
  return {
    projectId: "project-1",
    projectDisplayName: "project",
    projectRootPath: PROJECT_PATH,
    projectKind: "git" as const,
  };
}

interface RecordedProject {
  serverId: string;
  project: ProjectDescriptor;
}

interface RecordedHydrated {
  serverId: string;
  hydrated: boolean;
}

interface RecordedClone {
  repo: string;
  targetDirectory: string;
  cloneProtocol?: "https" | "ssh";
  progress?: boolean;
}

function createFakeSession() {
  const projects: RecordedProject[] = [];
  const hydrated: RecordedHydrated[] = [];
  return {
    projects,
    hydrated,
    upsertProject: (serverId: string, project: ProjectDescriptor) => {
      projects.push({ serverId, project });
    },
    setHasHydratedWorkspaces: (serverId: string, value: boolean) => {
      hydrated.push({ serverId, hydrated: value });
    },
  };
}

function createFakeGithubCloneClient(project: ReturnType<typeof buildProjectPayload> | null): {
  clones: RecordedClone[];
  listenerCount: () => number;
  listenForProgress: GithubCloneProgressListener;
  cloneGithubProject: (
    input: RecordedClone,
    requestId?: string,
  ) => Promise<{
    requestId: string;
    repo: string;
    checkoutPath: string;
    error: string | null;
    project: ReturnType<typeof buildProjectPayload> | null;
  }>;
} {
  const clones: RecordedClone[] = [];
  const listeners = new Set<(message: ProjectGithubCloneProgressMessage) => void>();
  const progressMessage = (
    requestId: string,
    percent: number,
  ): ProjectGithubCloneProgressMessage => ({
    type: "project.github.clone.progress",
    payload: { requestId, repo: "owner/project", phase: "receiving", percent },
  });
  return {
    clones,
    listenerCount: () => listeners.size,
    listenForProgress: (handler) => {
      listeners.add(handler);
      return () => {
        listeners.delete(handler);
      };
    },
    cloneGithubProject: async (input: RecordedClone, requestId?: string) => {
      clones.push(input);
      for (const listener of listeners) {
        listener(progressMessage("another-request", 10));
        if (requestId) listener(progressMessage(requestId, 44));
      }
      return {
        requestId: requestId ?? "request-3",
        repo: "owner/project",
        checkoutPath: PROJECT_PATH,
        error: project ? null : "Project registration failed",
        project,
      };
    },
  };
}

describe("openProjectDirectly", () => {
  it("adds the project and marks workspaces hydrated without opening a workspace", async () => {
    const session = createFakeSession();
    const projectPayload = buildProjectPayload();

    const result = await openProjectDirectly({
      serverId: SERVER_ID,
      projectPath: PROJECT_PATH,
      isConnected: true,
      canAddProject: true,
      client: {
        addProject: async () => ({
          requestId: "request-1",
          error: null,
          project: projectPayload,
        }),
      },
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({ ok: true, project: projectPayload });
    expect(session.projects).toEqual([
      {
        serverId: SERVER_ID,
        project: {
          projectId: "project-1",
          projectKey: null,
          projectDisplayName: "project",
          projectCustomName: null,
          projectCustomIconRevision: null,
          projectKind: "git",
          projectRootPath: PROJECT_PATH,
        },
      },
    ]);
    expect(session.hydrated).toEqual([{ serverId: SERVER_ID, hydrated: true }]);
  });

  it("fails before sending when the host does not support adding projects without workspaces", async () => {
    const session = createFakeSession();
    const result = await openProjectDirectly({
      serverId: SERVER_ID,
      projectPath: PROJECT_PATH,
      isConnected: true,
      canAddProject: false,
      client: {
        addProject: async () => ({
          requestId: "request-unsupported",
          error: null,
          project: buildProjectPayload(),
        }),
      },
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({
      ok: false,
      errorCode: null,
      error: "Update the host to add projects without creating a workspace.",
    });
    expect(session.projects).toEqual([]);
    expect(session.hydrated).toEqual([]);
  });

  it("does not add a project when addProject fails", async () => {
    const session = createFakeSession();

    const result = await openProjectDirectly({
      serverId: SERVER_ID,
      projectPath: PROJECT_PATH,
      isConnected: true,
      canAddProject: true,
      client: {
        addProject: async () => ({
          requestId: "request-2",
          error: "Directory not found: /repo/project",
          errorCode: "directory_not_found" as const,
          project: null,
        }),
      },
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({
      ok: false,
      errorCode: "directory_not_found",
      error: "Directory not found: /repo/project",
    });
    expect(session.projects).toEqual([]);
    expect(session.hydrated).toEqual([]);
  });
});

describe("cloneGithubProjectDirectly", () => {
  it("registers a cloned GitHub project without creating a workspace", async () => {
    const session = createFakeSession();
    const projectPayload = buildProjectPayload();
    const github = createFakeGithubCloneClient(projectPayload);

    const result = await cloneGithubProjectDirectly({
      serverId: SERVER_ID,
      repo: "owner/project",
      targetDirectory: "~/workspace",
      cloneProtocol: "https",
      isConnected: true,
      client: github,
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({ ok: true, project: projectPayload });
    expect(github.clones).toEqual([
      {
        repo: "owner/project",
        targetDirectory: "~/workspace",
        cloneProtocol: "https",
      },
    ]);
    expect(session.projects).toEqual([
      {
        serverId: SERVER_ID,
        project: {
          ...projectPayload,
          projectCustomName: null,
          projectKey: null,
          projectCustomIconRevision: null,
        },
      },
    ]);
    expect(session.hydrated).toEqual([{ serverId: SERVER_ID, hydrated: true }]);
  });

  it("does not register a project when cloning fails", async () => {
    const session = createFakeSession();
    const github = createFakeGithubCloneClient(null);

    const result = await cloneGithubProjectDirectly({
      serverId: SERVER_ID,
      repo: "owner/project",
      targetDirectory: "~/workspace",
      cloneProtocol: "https",
      isConnected: true,
      client: github,
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({
      ok: false,
      errorCode: null,
      error: "Project registration failed",
    });
    expect(session.projects).toEqual([]);
    expect(session.hydrated).toEqual([]);
  });

  it("reports only its own live clone progress and unsubscribes afterwards", async () => {
    const session = createFakeSession();
    const projectPayload = buildProjectPayload();
    const github = createFakeGithubCloneClient(projectPayload);
    const progress: string[] = [];

    const result = await cloneGithubProjectDirectly({
      serverId: SERVER_ID,
      repo: "owner/project",
      targetDirectory: "~/workspace",
      isConnected: true,
      client: github,
      listenForProgress: github.listenForProgress,
      onProgress: (update) => progress.push(`${update.phase}:${update.percent}`),
      upsertProject: session.upsertProject,
      setHasHydratedWorkspaces: session.setHasHydratedWorkspaces,
    });

    expect(result).toEqual({ ok: true, project: projectPayload });
    expect(github.clones).toEqual([
      {
        repo: "owner/project",
        targetDirectory: "~/workspace",
        progress: true,
      },
    ]);
    expect(progress).toEqual(["receiving:44"]);
    expect(github.listenerCount()).toBe(0);
  });
});

describe("getOpenProjectFailureReason", () => {
  it("keeps the known directory-not-found failure reason", () => {
    expect(
      getOpenProjectFailureReason({
        ok: false,
        errorCode: "directory_not_found",
        error: "Directory not found: /missing",
      }),
    ).toBe("directory_not_found");
  });

  it("uses the generic failure reason for untyped project-open failures", () => {
    expect(getOpenProjectFailureReason({ ok: false, errorCode: null, error: "boom" })).toBe(
      "open_failed",
    );
  });
});
