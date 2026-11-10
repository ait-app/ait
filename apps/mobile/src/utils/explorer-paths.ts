import { isAbsolutePath } from "./path";

interface BuildAbsoluteExplorerPathInput {
  workspaceRoot: string;
  entryPath: string;
}

export function buildAbsoluteExplorerPath({
  workspaceRoot,
  entryPath,
}: BuildAbsoluteExplorerPathInput): string {
  const trimmedWorkspaceRoot = workspaceRoot.trim();
  const normalizedWorkspaceRoot = trimmedWorkspaceRoot.replace(/[\\/]+$/, "");
  const normalizedEntryPath = entryPath.trim();
  const isFilesystemRoot = !normalizedWorkspaceRoot && trimmedWorkspaceRoot.startsWith("/");

  if (!normalizedWorkspaceRoot && !isFilesystemRoot) {
    return normalizedEntryPath;
  }

  const workspaceRootPath = isFilesystemRoot ? "/" : normalizedWorkspaceRoot;

  if (!normalizedEntryPath || normalizedEntryPath === ".") {
    return workspaceRootPath;
  }

  if (isAbsolutePath(normalizedEntryPath)) {
    return normalizedEntryPath;
  }

  const separator = normalizedWorkspaceRoot.includes("\\") ? "\\" : "/";
  const segments = normalizedEntryPath.split(/[\\/]+/).filter(Boolean);
  if (segments.length === 0) {
    return workspaceRootPath;
  }

  return `${isFilesystemRoot ? "" : normalizedWorkspaceRoot}${separator}${segments.join(separator)}`;
}

export function parentExplorerPath(entryPath: string): string {
  const separatorIndex = entryPath.lastIndexOf("/");
  return separatorIndex > 0 ? entryPath.slice(0, separatorIndex) : ".";
}

export function isExplorerPathWithin(candidatePath: string, parentPath: string): boolean {
  return candidatePath === parentPath || candidatePath.startsWith(`${parentPath}/`);
}

export function pruneExplorerPathsWithin(paths: readonly string[], parentPath: string): string[] {
  return paths.filter((path) => !isExplorerPathWithin(path, parentPath));
}

export function pruneDeletedExplorerEntry<TDirectory>(
  state: { directories: ReadonlyMap<string, TDirectory>; selectedEntryPath: string | null },
  deletedPath: string,
): { directories: Map<string, TDirectory>; selectedEntryPath: string | null } {
  const directories = new Map(state.directories);
  for (const path of state.directories.keys()) {
    if (isExplorerPathWithin(path, deletedPath)) {
      directories.delete(path);
    }
  }
  const { selectedEntryPath } = state;
  return {
    directories,
    selectedEntryPath:
      selectedEntryPath && isExplorerPathWithin(selectedEntryPath, deletedPath)
        ? null
        : selectedEntryPath,
  };
}
