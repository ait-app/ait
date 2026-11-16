import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type {
  ProjectGithubCloneProgressMessage,
  ProjectGithubCloneProtocol,
  ProjectAddResponse,
  WorkspaceProjectDescriptorPayload,
} from "@ait/protocol/messages";
import { normalizeProjectDescriptor, type ProjectDescriptor } from "@/stores/session-store";
import { generateMessageId } from "@/types/stream";

type OpenProjectPayload = ProjectAddResponse["payload"];
type OpenProjectErrorCode = NonNullable<OpenProjectPayload["errorCode"]>;

/** Live stage and overall completion reported while a GitHub clone runs. */
export type GithubCloneProgress = ProjectGithubCloneProgressMessage["payload"];

interface OpenProjectSuccess {
  ok: true;
  project: WorkspaceProjectDescriptorPayload;
}

interface OpenProjectFailure {
  ok: false;
  errorCode: OpenProjectErrorCode | null;
  error: string | null;
}

export type OpenProjectResult = OpenProjectSuccess | OpenProjectFailure;
type OpenProjectFailureReason = "directory_not_found" | "open_failed";
export type { ProjectGithubCloneProtocol };

export function getOpenProjectFailureReason(
  result: OpenProjectResult,
): OpenProjectFailureReason | null {
  if (result.ok) {
    return null;
  }

  if (result.errorCode === "directory_not_found") {
    return "directory_not_found";
  }

  return "open_failed";
}

interface OpenProjectDirectlyInput {
  serverId: string;
  projectPath: string;
  isConnected: boolean;
  canAddProject: boolean;
  client: Pick<DaemonClient, "addProject"> | null;
  upsertProject: (serverId: string, project: ProjectDescriptor) => void;
  setHasHydratedWorkspaces: (serverId: string, hydrated: boolean) => void;
}

interface ProjectRegistrationCallbacks {
  serverId: string;
  isConnected: boolean;
  upsertProject: (serverId: string, project: ProjectDescriptor) => void;
  setHasHydratedWorkspaces: (serverId: string, hydrated: boolean) => void;
}

interface RegisterProjectDescriptorInput {
  serverId: string;
  project: WorkspaceProjectDescriptorPayload;
  upsertProject: (serverId: string, project: ProjectDescriptor) => void;
  setHasHydratedWorkspaces: (serverId: string, hydrated: boolean) => void;
}

export function registerProjectDescriptor(input: RegisterProjectDescriptorInput): boolean {
  const serverId = input.serverId.trim();
  if (!serverId) return false;
  input.upsertProject(serverId, normalizeProjectDescriptor(input.project));
  input.setHasHydratedWorkspaces(serverId, true);
  return true;
}

interface CloneGithubProjectDirectlyInput extends ProjectRegistrationCallbacks {
  repo: string;
  targetDirectory: string;
  cloneProtocol?: ProjectGithubCloneProtocol;
  client: Pick<DaemonClient, "cloneGithubProject"> | null;
  listenForProgress?: GithubCloneProgressListener;
  onProgress?: (progress: GithubCloneProgress) => void;
}

/** Register one handler for clone progress events on the host connection. */
export type GithubCloneProgressListener = (
  handler: (message: ProjectGithubCloneProgressMessage) => void,
) => () => void;

export async function openProjectDirectly(
  input: OpenProjectDirectlyInput,
): Promise<OpenProjectResult> {
  const normalizedServerId = input.serverId.trim();
  const trimmedPath = input.projectPath.trim();
  if (!normalizedServerId || !trimmedPath || !input.client || !input.isConnected) {
    return { ok: false, errorCode: null, error: null };
  }

  if (!input.canAddProject) {
    return {
      ok: false,
      errorCode: null,
      error: "Update the host to add projects without creating a workspace.",
    };
  }

  const payload = await input.client.addProject(trimmedPath);
  if (payload.error || !payload.project) {
    return {
      ok: false,
      errorCode: payload.errorCode ?? null,
      error: payload.error,
    };
  }

  const registered = registerProjectDescriptor({
    serverId: normalizedServerId,
    project: payload.project,
    upsertProject: input.upsertProject,
    setHasHydratedWorkspaces: input.setHasHydratedWorkspaces,
  });
  return registered
    ? { ok: true, project: payload.project }
    : { ok: false, errorCode: null, error: "Unable to register project" };
}

export async function cloneGithubProjectDirectly(
  input: CloneGithubProjectDirectlyInput,
): Promise<OpenProjectResult> {
  const normalizedServerId = input.serverId.trim();
  const trimmedRepo = input.repo.trim();
  const trimmedTargetDirectory = input.targetDirectory.trim();
  if (
    !normalizedServerId ||
    !trimmedRepo ||
    !trimmedTargetDirectory ||
    !input.client ||
    !input.isConnected
  ) {
    return { ok: false, errorCode: null, error: null };
  }

  const requestId = `github_clone_${generateMessageId()}`;
  const onProgress = input.onProgress;
  const unsubscribe =
    onProgress && input.listenForProgress
      ? input.listenForProgress((message) => {
          if (message.payload.requestId !== requestId) return;
          onProgress(message.payload);
        })
      : null;
  try {
    const payload = await input.client.cloneGithubProject(
      {
        repo: trimmedRepo,
        targetDirectory: trimmedTargetDirectory,
        ...(input.cloneProtocol ? { cloneProtocol: input.cloneProtocol } : {}),
        ...(onProgress && input.listenForProgress ? { progress: true } : {}),
      },
      requestId,
    );
    if (payload.error || !payload.project) {
      return { ok: false, errorCode: null, error: payload.error };
    }

    const registered = registerProjectDescriptor({
      serverId: normalizedServerId,
      project: payload.project,
      upsertProject: input.upsertProject,
      setHasHydratedWorkspaces: input.setHasHydratedWorkspaces,
    });
    return registered
      ? { ok: true, project: payload.project }
      : { ok: false, errorCode: null, error: "Unable to register project" };
  } finally {
    unsubscribe?.();
  }
}
