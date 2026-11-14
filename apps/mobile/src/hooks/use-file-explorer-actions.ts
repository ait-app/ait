import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  useSessionStore,
  type AgentFileExplorerState,
  type ExplorerDirectory,
} from "@/stores/session-store";
import { explorerFileFromReadResult } from "@/file-explorer/read-result";
import { usePanelStore } from "@/stores/panel-store";
import { revealExplorerDirectory } from "@/file-explorer/tree";
import { parentExplorerPath } from "@/utils/explorer-paths";
import { trimNonEmpty } from "@/utils/string";

function createExplorerState(): AgentFileExplorerState {
  return {
    directories: new Map(),
    files: new Map(),
    isLoading: false,
    lastError: null,
    pendingRequest: null,
    currentPath: ".",
    history: ["."],
    lastVisitedPath: ".",
    selectedEntryPath: null,
  };
}

function pushHistory(history: string[], path: string): string[] {
  const normalizedHistory = history.length === 0 ? ["."] : history;
  const last = normalizedHistory[normalizedHistory.length - 1];
  if (last === path) {
    return normalizedHistory;
  }
  return [...normalizedHistory, path];
}

interface FileExplorerWorkspaceScope {
  workspaceId?: string | null;
  workspaceRoot?: string | null;
}

export function buildWorkspaceExplorerStateKey(scope: FileExplorerWorkspaceScope): string | null {
  const normalizedWorkspaceId = trimNonEmpty(scope.workspaceId);
  if (normalizedWorkspaceId) {
    return `workspace:${normalizedWorkspaceId}`;
  }
  const normalizedWorkspaceRoot = trimNonEmpty(scope.workspaceRoot);
  if (!normalizedWorkspaceRoot) {
    return null;
  }
  return `root:${normalizedWorkspaceRoot}`;
}

export function useFileExplorerActions(params: { serverId: string } & FileExplorerWorkspaceScope) {
  const { t } = useTranslation();
  const { serverId, workspaceId, workspaceRoot } = params;
  const client = useSessionStore((state) => state.sessions[serverId]?.client ?? null);
  const setFileExplorer = useSessionStore((state) => state.setFileExplorer);
  const normalizedWorkspaceRoot = useMemo(() => trimNonEmpty(workspaceRoot), [workspaceRoot]);
  const workspaceStateKey = useMemo(
    () =>
      buildWorkspaceExplorerStateKey({
        workspaceId,
        workspaceRoot: normalizedWorkspaceRoot,
      }),
    [workspaceId, normalizedWorkspaceRoot],
  );

  const updateExplorerState = useCallback(
    (updater: (prev: AgentFileExplorerState) => AgentFileExplorerState) => {
      if (!workspaceStateKey) {
        return;
      }
      setFileExplorer(serverId, (prev) => {
        const next = new Map(prev);
        const current = next.get(workspaceStateKey) ?? createExplorerState();
        next.set(workspaceStateKey, updater(current));
        return next;
      });
    },
    [serverId, setFileExplorer, workspaceStateKey],
  );

  const failExplorerRequest = useCallback(
    (message: string) => {
      updateExplorerState((state) => ({
        ...state,
        isLoading: false,
        lastError: message,
        pendingRequest: null,
      }));
    },
    [updateExplorerState],
  );

  const requestDirectoryListing = useCallback(
    async (
      path: string,
      options?: { recordHistory?: boolean; setCurrentPath?: boolean },
    ): Promise<ExplorerDirectory | null> => {
      if (!workspaceStateKey) {
        return null;
      }
      const normalizedPath = path && path.length > 0 ? path : ".";
      const shouldSetCurrentPath = options?.setCurrentPath ?? true;
      const shouldRecordHistory = options?.recordHistory ?? shouldSetCurrentPath;

      updateExplorerState((state) => ({
        ...state,
        isLoading: true,
        lastError: null,
        pendingRequest: { path: normalizedPath, mode: "list" },
        ...(shouldSetCurrentPath
          ? {
              currentPath: normalizedPath,
              history: shouldRecordHistory
                ? pushHistory(state.history, normalizedPath)
                : state.history,
              lastVisitedPath: normalizedPath,
            }
          : {}),
      }));

      if (!normalizedWorkspaceRoot) {
        failExplorerRequest(t("workspace.fileExplorer.states.unavailable"));
        return null;
      }

      if (!client) {
        failExplorerRequest(t("workspace.terminal.hostDisconnected"));
        return null;
      }

      try {
        const directory = await client.listDirectory(normalizedWorkspaceRoot, normalizedPath);
        updateExplorerState((state) => ({
          ...state,
          isLoading: false,
          lastError: null,
          pendingRequest: null,
          directories: new Map(state.directories).set(directory.path, directory),
        }));
        return directory;
      } catch (error) {
        failExplorerRequest(
          error instanceof Error
            ? error.message
            : t("workspace.fileExplorer.errors.failedToListDirectory"),
        );
        return null;
      }
    },
    [
      client,
      failExplorerRequest,
      normalizedWorkspaceRoot,
      t,
      updateExplorerState,
      workspaceStateKey,
    ],
  );

  const revealDirectory = useCallback(
    async (directoryPath: string) => {
      if (!workspaceStateKey) return;
      await revealExplorerDirectory({
        directoryPath,
        requestDirectoryListing,
        showHiddenFiles: () => {
          const panel = usePanelStore.getState();
          if (!panel.explorerShowHiddenFiles) panel.toggleExplorerShowHiddenFiles();
        },
        expandPaths: (paths) =>
          usePanelStore
            .getState()
            .setExpandedPathsForWorkspace(workspaceStateKey, (current) =>
              Array.from(new Set([...current, ...paths])),
            ),
      });
    },
    [requestDirectoryListing, workspaceStateKey],
  );

  const requestFilePreview = useCallback(
    async (path: string) => {
      if (!workspaceStateKey) {
        return;
      }
      const normalizedPath = path && path.length > 0 ? path : ".";
      updateExplorerState((state) => ({
        ...state,
        isLoading: true,
        lastError: null,
        pendingRequest: { path: normalizedPath, mode: "file" },
      }));

      if (!normalizedWorkspaceRoot) {
        failExplorerRequest(t("workspace.fileExplorer.states.unavailable"));
        return;
      }

      if (!client) {
        failExplorerRequest(t("workspace.terminal.hostDisconnected"));
        return;
      }

      try {
        const file = await client.readFile(normalizedWorkspaceRoot, normalizedPath);
        const explorerFile = explorerFileFromReadResult(file);
        updateExplorerState((state) => ({
          ...state,
          isLoading: false,
          lastError: null,
          pendingRequest: null,
          files: new Map(state.files).set(explorerFile.path, explorerFile),
        }));
      } catch (error) {
        failExplorerRequest(
          error instanceof Error ? error.message : t("panels.file.failedToLoadPreview"),
        );
      }
    },
    [
      client,
      failExplorerRequest,
      normalizedWorkspaceRoot,
      t,
      updateExplorerState,
      workspaceStateKey,
    ],
  );

  const requestFileDownloadToken = useCallback(
    async (path: string) => {
      if (!normalizedWorkspaceRoot) {
        throw new Error(t("workspace.fileExplorer.states.unavailable"));
      }
      if (!client) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      const payload = await client.requestDownloadToken(normalizedWorkspaceRoot, path);
      if (payload.error) {
        throw new Error(payload.error);
      }
      return payload;
    },
    [client, normalizedWorkspaceRoot, t],
  );

  const requestFileBytes = useCallback(
    async (path: string, maxBytes: number) => {
      if (!normalizedWorkspaceRoot) {
        throw new Error(t("workspace.fileExplorer.states.unavailable"));
      }
      if (!client) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      return client.readFile(normalizedWorkspaceRoot, path, undefined, maxBytes);
    },
    [client, normalizedWorkspaceRoot, t],
  );

  const createEntry = useCallback(
    async (input: { parentPath: string; name: string; kind: "file" | "directory" }) => {
      if (!client || !normalizedWorkspaceRoot) {
        return null;
      }
      const payload = await client.createFileEntry({
        cwd: normalizedWorkspaceRoot,
        ...input,
      });
      if (payload.success) {
        await requestDirectoryListing(input.parentPath, {
          recordHistory: false,
          setCurrentPath: false,
        });
      }
      return payload;
    },
    [client, normalizedWorkspaceRoot, requestDirectoryListing],
  );

  const renameEntry = useCallback(
    async (input: { path: string; name: string }) => {
      if (!client || !normalizedWorkspaceRoot) {
        return null;
      }
      const payload = await client.renameFileEntry({
        cwd: normalizedWorkspaceRoot,
        ...input,
      });
      if (payload.success) {
        await requestDirectoryListing(parentExplorerPath(input.path), {
          recordHistory: false,
          setCurrentPath: false,
        });
      }
      return payload;
    },
    [client, normalizedWorkspaceRoot, requestDirectoryListing],
  );

  const duplicateEntry = useCallback(
    async (path: string) => {
      if (!client || !normalizedWorkspaceRoot) {
        return null;
      }
      const payload = await client.duplicateFileEntry({ cwd: normalizedWorkspaceRoot, path });
      if (payload.success) {
        await requestDirectoryListing(parentExplorerPath(path), {
          recordHistory: false,
          setCurrentPath: false,
        });
      }
      return payload;
    },
    [client, normalizedWorkspaceRoot, requestDirectoryListing],
  );

  const deleteEntry = useCallback(
    async (path: string) => {
      if (!client || !normalizedWorkspaceRoot) {
        return null;
      }
      const payload = await client.deleteFileEntry({ cwd: normalizedWorkspaceRoot, path });
      if (payload.success) {
        await requestDirectoryListing(parentExplorerPath(path), {
          recordHistory: false,
          setCurrentPath: false,
        });
      }
      return payload;
    },
    [client, normalizedWorkspaceRoot, requestDirectoryListing],
  );

  const selectExplorerEntry = useCallback(
    (path: string | null) => {
      updateExplorerState((state) => ({
        ...state,
        selectedEntryPath: path,
      }));
    },
    [updateExplorerState],
  );

  return {
    workspaceStateKey,
    revealDirectory,
    requestDirectoryListing,
    requestFilePreview,
    requestFileDownloadToken,
    requestFileBytes,
    createEntry,
    renameEntry,
    duplicateEntry,
    deleteEntry,
    selectExplorerEntry,
  };
}
