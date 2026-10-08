import type { ProjectDescriptor, WorkspaceDescriptor } from "@/stores/session-store";
import { projectDisplayNameFromProjectId } from "@/utils/project-display-name";

export interface WorkspaceStructureHostPlacement {
  serverId: string;
  projectId: string;
  iconWorkingDir: string;
  worktreeSupport: "supported" | "unsupported" | "unknown";
  customIconRevision?: string | null;
  iconRevision?: string;
}

export interface WorkspaceStructureProject {
  viewKey: string;
  projectKey: string | null;
  projectName: string;
  projectKind: WorkspaceDescriptor["projectKind"] | "unknown";
  iconWorkingDir: string;
  hosts: WorkspaceStructureHostPlacement[];
  workspaceKeys: string[];
}

export interface WorkspaceStructure {
  projects: WorkspaceStructureProject[];
}

interface WorkspaceStructureSession {
  serverId: string;
  projects: Iterable<ProjectDescriptor>;
  workspaces: Iterable<WorkspaceDescriptor>;
}

interface ProjectDraft {
  viewKey: string;
  projectKey: string | null;
  projectName: string;
  projectKind: WorkspaceDescriptor["projectKind"];
  iconWorkingDir: string;
  hosts: Map<string, WorkspaceStructureHostPlacement>;
  workspaces: Array<{ workspaceId: string; workspaceName: string; workspaceKey: string }>;
}

/** Build display projects by host-local identity, independently of repository equivalence. */
export function buildWorkspaceStructureProjects(input: {
  sessions: WorkspaceStructureSession[];
}): WorkspaceStructureProject[] {
  const byProject = new Map<string, ProjectDraft>();
  const viewKeyByServerProjectId = new Map<string, Map<string, string>>();

  for (const session of input.sessions) {
    for (const project of session.projects) {
      const viewKey = addProjectToView({ byProject, serverId: session.serverId, project });
      getOrCreate(viewKeyByServerProjectId, session.serverId, () => new Map()).set(
        project.projectId,
        viewKey,
      );
    }
  }

  for (const session of input.sessions) {
    for (const workspace of session.workspaces) {
      const viewKey = viewKeyByServerProjectId.get(session.serverId)?.get(workspace.projectId);
      if (!viewKey) continue;
      byProject.get(viewKey)?.workspaces.push({
        workspaceId: workspace.id,
        workspaceName: workspace.name,
        workspaceKey: `${session.serverId}:${workspace.id}`,
      });
    }
  }

  return Array.from(byProject.values())
    .map((draft) => ({
      viewKey: draft.viewKey,
      projectKey: draft.projectKey,
      projectName: draft.projectName,
      projectKind: draft.projectKind,
      iconWorkingDir: draft.iconWorkingDir,
      hosts: Array.from(draft.hosts.values()),
      workspaceKeys: draft.workspaces
        .sort(compareWorkspaceStructureItems)
        .map((workspace) => workspace.workspaceKey),
    }))
    .sort(
      (left, right) =>
        left.projectName.localeCompare(right.projectName, undefined, {
          numeric: true,
          sensitivity: "base",
        }) || left.viewKey.localeCompare(right.viewKey),
    );
}

export function createProjectViewKey(identity: {
  kind: "placement";
  serverId: string;
  projectId: string;
}): string {
  return JSON.stringify([identity.serverId, identity.projectId]);
}

function addProjectToView(input: {
  byProject: Map<string, ProjectDraft>;
  serverId: string;
  project: ProjectDescriptor;
}): string {
  const { byProject, serverId, project } = input;
  const viewKey = createProjectViewKey({
    kind: "placement",
    serverId,
    projectId: project.projectId,
  });
  const placement: WorkspaceStructureHostPlacement = {
    serverId,
    projectId: project.projectId,
    iconWorkingDir: project.projectRootPath,
    worktreeSupport: project.projectKind === "git" ? "supported" : "unsupported",
    customIconRevision: project.projectCustomIconRevision,
    iconRevision: project.projectIconRevision,
  };
  byProject.set(viewKey, {
    viewKey,
    projectKey: project.projectKey ?? null,
    projectName:
      project.projectCustomName ??
      project.projectDisplayName ??
      projectDisplayNameFromProjectId(project.projectId),
    projectKind: project.projectKind,
    iconWorkingDir: project.projectRootPath,
    hosts: new Map([[serverId, placement]]),
    workspaces: [],
  });
  return viewKey;
}

function getOrCreate<K, V>(map: Map<K, V>, key: K, create: () => V): V {
  const existing = map.get(key);
  if (existing !== undefined) return existing;
  const value = create();
  map.set(key, value);
  return value;
}

function compareWorkspaceStructureItems(
  left: { workspaceId: string; workspaceName: string },
  right: { workspaceId: string; workspaceName: string },
): number {
  return (
    left.workspaceName.localeCompare(right.workspaceName, undefined, {
      numeric: true,
      sensitivity: "base",
    }) || left.workspaceId.localeCompare(right.workspaceId, undefined, { sensitivity: "base" })
  );
}
