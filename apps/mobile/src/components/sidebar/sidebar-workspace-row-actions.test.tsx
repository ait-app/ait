// @vitest-environment jsdom
import React from "react";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import {
  useSidebarWorkspaceRowActions,
  type SidebarWorkspaceRowActions,
} from "./sidebar-workspace-row-actions";

const { toastError, readState } = vi.hoisted(() => ({
  toastError: vi.fn(),
  readState: {
    clearAttention: vi.fn<() => Promise<void>>(),
    markUnread: vi.fn<() => Promise<void>>(),
  },
}));

vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: toastError }) }));
vi.mock("@/workspace/use-workspace-archive", () => ({
  useWorkspaceArchive: () => ({ archive: vi.fn() }),
}));
vi.mock("@/hooks/use-workspace-clipboard-actions", () => ({
  useWorkspaceClipboardActions: () => ({ copyPath: vi.fn(), copyBranchName: vi.fn() }),
}));
vi.mock("@/hooks/use-workspace-read-state", () => ({
  useWorkspaceReadState: () => ({
    hasClearableAttention: true,
    canMarkUnread: true,
    clearAttention: readState.clearAttention,
    markUnread: readState.markUnread,
  }),
}));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({ useKeyboardActionHandler: () => {} }));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => null }));
vi.mock("@/components/workspace-rename-modal", () => ({ WorkspaceRenameModal: () => null }));
vi.mock("@/components/sharing/share-manager-sheet", () => ({
  ShareManagerSheet: ({
    visible,
    target,
  }: {
    visible: boolean;
    target: { resourceKind: string; resourceKey: string } | null;
  }) =>
    visible && target ? (
      <div data-testid="share-sheet">{`${target.resourceKind}:${target.resourceKey}`}</div>
    ) : null,
}));
vi.mock("@/components/sidebar/sidebar-workspace-menu", () => ({
  SidebarWorkspaceMenu: () => null,
}));
vi.mock("@/components/sidebar/sidebar-workspace-row-content", () => ({}));
vi.mock("@/components/sidebar/workspace-trailing", () => ({}));

const workspace: SidebarWorkspaceEntry = {
  workspaceKey: "host:workspace",
  serverId: "host",
  workspaceId: "workspace",
  projectViewKey: "project",
  projectName: "project",
  projectRootPath: "/project",
  projectKind: "git",
  workspaceKind: "worktree",
  name: "workspace",
  workspaceDirectory: "/project/workspace",
  workspaceDirectoryLabel: "workspace",
  title: null,
  currentBranch: null,
  statusBucket: "done",
  statusEnteredAt: null,
  archivingAt: null,
  diffStat: null,
  prHint: null,
  archiveHasUncommittedChanges: null,
  archiveUnpushedCommitCount: null,
  scripts: [],
  hasRunningScripts: false,
};

let latestActions: SidebarWorkspaceRowActions | null = null;

function Row() {
  const { actions, shareSheet } = useSidebarWorkspaceRowActions({
    workspace,
    selected: false,
    canCopyBranchName: true,
    canPin: true,
    onToggleWorkspacePin: () => {},
  });
  latestActions = actions;
  return shareSheet;
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  latestActions = null;
});

afterEach(async () => {
  cleanup();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  await i18n.changeLanguage("en");
});

describe("sidebar workspace row actions", () => {
  it("give every row Share and the workspace folder, whichever list renders it", () => {
    render(<Row />);
    expect(latestActions?.openInFileManagerPath).toBe("/project/workspace");
    expect(latestActions?.onShare).toBeTypeOf("function");

    act(() => latestActions?.onShare?.());
    expect(screen.getByTestId("share-sheet").textContent).toBe("workspace:workspace");
  });

  it("translates the fallback toasts when marking read or unread fails", async () => {
    await i18n.changeLanguage("zh-CN");
    readState.clearAttention.mockRejectedValue("offline");
    readState.markUnread.mockRejectedValue("offline");
    render(<Row />);

    act(() => latestActions?.onMarkAsRead?.());
    await waitFor(() =>
      expect(toastError).toHaveBeenLastCalledWith(
        i18n.t("sidebar.workspace.toasts.markAsReadFailed", { lng: "zh-CN" }),
      ),
    );
    act(() => latestActions?.onMarkAsUnread?.());
    await waitFor(() =>
      expect(toastError).toHaveBeenLastCalledWith(
        i18n.t("sidebar.workspace.toasts.markAsUnreadFailed", { lng: "zh-CN" }),
      ),
    );
  });

  it("keeps a thrown error's own message", async () => {
    readState.clearAttention.mockRejectedValue(new Error("Host is not connected"));
    render(<Row />);
    act(() => latestActions?.onMarkAsRead?.());
    await waitFor(() => expect(toastError).toHaveBeenLastCalledWith("Host is not connected"));
  });
});
