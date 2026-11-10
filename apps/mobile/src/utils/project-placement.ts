import { projectDisplayNameFromProjectId } from "@/utils/project-display-name";
import type { ProjectPlacementPayload } from "@ait/protocol/messages";

function normalizeWorkingDirectory(cwd: string): string {
  const trimmed = cwd.trim();
  return trimmed.length > 0 ? trimmed : ".";
}

export function deriveProjectPlacementFromCwd(cwd: string): ProjectPlacementPayload {
  const normalizedCwd = normalizeWorkingDirectory(cwd);
  const projectKey = normalizedCwd;

  return {
    projectKey,
    projectName: projectDisplayNameFromProjectId(projectKey),
    workspaceName: null,
    checkout: {
      cwd: normalizedCwd,
      isGit: false,
      currentBranch: null,
      remoteUrl: null,
      worktreeRoot: null,
      isPaseoOwnedWorktree: false,
      mainRepoRoot: null,
    },
  };
}

export function resolveProjectPlacement(input: {
  projectPlacement: ProjectPlacementPayload | null | undefined;
  cwd: string;
}): ProjectPlacementPayload {
  return input.projectPlacement ?? deriveProjectPlacementFromCwd(input.cwd);
}
