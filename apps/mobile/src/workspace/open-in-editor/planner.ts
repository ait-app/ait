import { type Forge, forgeFromRemoteUrl, getForgePresentation } from "@/git/forge";
import type { DesktopOpenTarget, OpenDesktopTargetInput } from "@/workspace/desktop-open-targets";
import {
  type ResolvedWorkspaceFilePaths,
  resolveWorkspaceFilePaths,
  type WorkspaceFileLocation,
} from "@/workspace/file-open";

interface CheckoutStatusForOpenTarget {
  isGit: boolean;
  remoteUrl?: string | null;
  currentBranch?: string | null;
}

interface PlannedDesktopOpenTarget {
  source: "desktop";
  id: string;
  label: string;
  editorId: string;
  icon: DesktopOpenTarget["icon"];
  openInput: OpenDesktopTargetInput;
}

interface PlannedForgeOpenTarget {
  source: "forge";
  forge: Forge;
  id: Forge;
  label: string;
  url: string;
}

type PlannedWorkspaceOpenTarget = PlannedDesktopOpenTarget | PlannedForgeOpenTarget;

interface PlanWorkspaceOpenTargetsInput {
  workspaceDirectory: string;
  directoryPath?: string | null;
  activeFile?: WorkspaceFileLocation | null;
  resolvedActiveFile?: ResolvedWorkspaceFilePaths | null;
  desktopTargets: readonly DesktopOpenTarget[];
  canUseDesktopBridge: boolean;
  isLocalExecution: boolean;
  checkoutStatus?: CheckoutStatusForOpenTarget | null;
  forge?: Forge | null;
}

function resolveActiveFileForOpenTargets(
  input: Pick<
    PlanWorkspaceOpenTargetsInput,
    "activeFile" | "resolvedActiveFile" | "workspaceDirectory"
  >,
): ResolvedWorkspaceFilePaths | null {
  if (input.resolvedActiveFile !== undefined) {
    return input.resolvedActiveFile;
  }
  return input.activeFile
    ? resolveWorkspaceFilePaths({
        path: input.activeFile.path,
        workspaceRoot: input.workspaceDirectory,
      })
    : null;
}

function planDesktopOpenTargets(input: {
  workspaceDirectory: string;
  directoryPath?: string | null;
  activeFile?: WorkspaceFileLocation | null;
  resolvedFile: ResolvedWorkspaceFilePaths | null;
  desktopTargets: readonly DesktopOpenTarget[];
  canUseDesktopBridge: boolean;
  isLocalExecution: boolean;
}): PlannedDesktopOpenTarget[] {
  if (!input.canUseDesktopBridge || !input.isLocalExecution) {
    return [];
  }

  let workspacePath = input.workspaceDirectory;
  if (input.directoryPath !== undefined && input.directoryPath !== null) {
    const resolvedDirectory = resolveWorkspaceFilePaths({
      path: input.directoryPath,
      workspaceRoot: input.workspaceDirectory,
    });
    if (!resolvedDirectory?.relativePath) {
      return [];
    }
    workspacePath = resolvedDirectory.absolutePath;
  }

  return input.desktopTargets.map((target) => ({
    source: "desktop",
    id: target.id,
    label: target.label,
    editorId: target.id,
    icon: target.icon,
    openInput: input.resolvedFile
      ? {
          editorId: target.id,
          workspacePath,
          filePath: input.resolvedFile.absolutePath,
          ...(input.activeFile?.lineStart ? { line: input.activeFile.lineStart } : {}),
        }
      : { editorId: target.id, workspacePath },
  }));
}

function buildForgeWebUrl(
  forge: Forge,
  input: {
    remoteUrl: string | null | undefined;
    branch: string | null | undefined;
    path: string | null;
    lineStart?: number;
    lineEnd?: number;
  },
): string | null {
  const presentation = getForgePresentation(forge);
  if (input.path) {
    return (
      presentation.buildBlobUrl?.({
        remoteUrl: input.remoteUrl,
        branch: input.branch,
        path: input.path,
        lineStart: input.lineStart,
        lineEnd: input.lineEnd,
      }) ?? null
    );
  }
  return (
    presentation.buildBranchTreeUrl?.({
      remoteUrl: input.remoteUrl,
      branch: input.branch,
    }) ?? null
  );
}

function planForgeOpenTarget(input: {
  activeFile?: WorkspaceFileLocation | null;
  resolvedFile: ResolvedWorkspaceFilePaths | null;
  checkoutStatus?: CheckoutStatusForOpenTarget | null;
  forge?: Forge | null;
}): PlannedForgeOpenTarget | null {
  if (!input.checkoutStatus?.isGit) {
    return null;
  }
  const forge = input.forge ?? forgeFromRemoteUrl(input.checkoutStatus.remoteUrl) ?? null;
  if (!forge) {
    return null;
  }
  const url = buildForgeWebUrl(forge, {
    remoteUrl: input.checkoutStatus.remoteUrl,
    branch: input.checkoutStatus.currentBranch,
    path: input.resolvedFile?.relativePath ?? null,
    lineStart: input.activeFile?.lineStart,
    lineEnd: input.activeFile?.lineEnd,
  });
  if (!url) {
    return null;
  }
  return {
    source: "forge",
    forge,
    id: forge,
    label: getForgePresentation(forge).brandLabel,
    url,
  };
}

export function planWorkspaceOpenTargets(
  input: PlanWorkspaceOpenTargetsInput,
): PlannedWorkspaceOpenTarget[] {
  const resolvedFile = resolveActiveFileForOpenTargets(input);
  const desktopTargets = planDesktopOpenTargets({ ...input, resolvedFile });
  const forgeTarget = planForgeOpenTarget({ ...input, resolvedFile });
  return forgeTarget ? [...desktopTargets, forgeTarget] : desktopTargets;
}
