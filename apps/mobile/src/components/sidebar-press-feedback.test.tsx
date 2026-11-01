// @vitest-environment jsdom
import React from "react";
import { act, cleanup, render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StyleSheet, View, type GestureResponderEvent } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PressHighlightProps } from "@/components/ui/press-highlight.types";
import type {
  SidebarProjectEntry,
  SidebarWorkspaceEntry,
} from "@/hooks/use-sidebar-workspaces-list";
import { SidebarWorkspaceList } from "./sidebar-workspace-list";
import { SidebarWorkspaceRow } from "./sidebar/sidebar-workspace-row";

const state = vi.hoisted(() => ({
  native: true,
  rows: new Map<string, PressHighlightProps>(),
}));

vi.mock("@/constants/platform", () => ({
  get isNative() {
    return state.native;
  },
  get isWeb() {
    return !state.native;
  },
  getIsElectron: () => false,
  getIsElectronMac: () => false,
}));
vi.mock("@/constants/layout", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/layout")>()),
  useIsCompactFormFactor: () => true,
}));
// Capture the underlying row surface independently of the native overlay. A failed Tap
// gesture clears that overlay on the UI thread before Pressable necessarily sends press-out.
vi.mock("@/components/ui/press-highlight", () => ({
  PressHighlight: (props: PressHighlightProps) => {
    if (props.testID) state.rows.set(props.testID, props);
    return <View testID={props.testID} />;
  },
}));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/runtime/host-runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/runtime/host-runtime")>()),
  useHosts: () => [],
}));
vi.mock("@/hooks/use-is-local-daemon", () => ({
  useLocalDaemonServerId: () => "host",
  useLocalDaemonServerIdState: () => ({ status: "resolved", serverId: "host" }),
}));
vi.mock("@/runtime/host-features", () => ({
  useHostFeatureMap: () => new Map(),
  useHostFeature: () => false,
}));
vi.mock("@/projects/icons", () => ({ useProjectIcons: () => new Map() }));
vi.mock("@/hooks/use-sidebar-workspace-pin", () => ({
  useSidebarWorkspacePinController: () => () => {},
}));
vi.mock("@/contexts/toast-context", () => ({ useToast: () => ({ error: vi.fn() }) }));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({ useKeyboardActionHandler: () => {} }));
vi.mock("@/components/workspace-hover-card", () => ({
  WorkspaceHoverCard: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock("@/components/sharing/share-manager-sheet", () => ({ ShareManagerSheet: () => null }));

const workspace: SidebarWorkspaceEntry = {
  workspaceKey: "host:workspace",
  serverId: "host",
  workspaceId: "workspace",
  projectViewKey: "project",
  projectName: "Project",
  projectKind: "git",
  workspaceKind: "worktree",
  name: "workspace",
  workspaceDirectory: "/repo/workspace",
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
const project: SidebarProjectEntry = {
  viewKey: "project",
  projectName: "Project",
  projectKind: "git",
  iconWorkingDir: "/repo",
  hosts: [
    {
      serverId: "host",
      projectId: "project",
      iconWorkingDir: "/repo",
      worktreeSupport: "supported",
    },
  ],
  workspaces: [workspace],
};

function Sidebar({ mode }: { mode: "project" | "status" }) {
  return (
    <SidebarWorkspaceList
      workspaceGroups={[
        {
          key: "done",
          label: "Done",
          rows: [workspace],
          leading: { kind: "status", bucket: "done" },
        },
      ]}
      projectIconTargets={[]}
      pinnedGroups={{ pinnedChats: [], unpinnedProjects: [project] }}
      projects={[project]}
      hasProjectsBeforeFilter
      hasActiveProjectFilter={false}
      workspaceEntriesByKey={new Map([[workspace.workspaceKey, workspace]])}
      collapsedProjectKeys={new Set()}
      onToggleProjectCollapsed={() => {}}
      shortcutIndexByWorkspaceKey={new Map()}
      groupMode={mode}
    />
  );
}

function renderRow(mode: "project" | "status" | "standalone") {
  const client = new QueryClient({ defaultOptions: { queries: { enabled: false, retry: false } } });
  return render(
    mode === "standalone" ? (
      <SidebarWorkspaceRow
        workspace={workspace}
        selected={false}
        shortcutNumber={null}
        showShortcutBadge={false}
        canCopyBranchName
        onPress={() => {}}
      />
    ) : (
      <Sidebar mode={mode} />
    ),
    {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    },
  );
}

function rowProps(id: string) {
  const props = state.rows.get(id);
  if (!props) throw new Error(`Missing row ${id}`);
  return props;
}
function background(id: string) {
  const style = rowProps(id).style;
  return StyleSheet.flatten(typeof style === "function" ? style({ pressed: false }) : style)
    ?.backgroundColor;
}
function touch(pageY: number): GestureResponderEvent {
  return { nativeEvent: { pageX: 20, pageY } } as GestureResponderEvent;
}

beforeEach(() => {
  state.native = true;
  state.rows.clear();
});
afterEach(cleanup);

describe.each([
  ["project header", "project", "sidebar-project-row-project"],
  ["project workspace", "project", "sidebar-workspace-row-host:workspace"],
  ["status workspace", "status", "sidebar-workspace-row-host:workspace"],
  ["standalone workspace", "standalone", "sidebar-workspace-row-host:workspace"],
] as const)("sidebar press feedback: %s", (_name, mode, id) => {
  it("does not retain a second native highlight after dragging and releasing without press-out", () => {
    renderRow(mode);
    const idleBackground = background(id);
    expect(rowProps(id).highlightStyle).toBeDefined();
    act(() => {
      rowProps(id).onPressIn?.(touch(20));
    });
    // Movement past the native Tap's 8pt tolerance clears its overlay, even while
    // the finger stays within the row's Pressable retention area.
    act(() => {
      rowProps(id).onTouchMove?.(touch(32));
    });
    act(() => {
      rowProps(id).onTouchCancel?.(touch(32));
    });
    act(() => {
      rowProps(id).onTouchEnd?.(touch(32));
    });
    expect(background(id)).toBe(idleBackground);
    act(() => {
      rowProps(id).onPressOut?.(touch(32));
    });
    expect(background(id)).toBe(idleBackground);
  });
  it("preserves web press feedback and clears it on release", () => {
    state.native = false;
    renderRow(mode);
    const idleBackground = background(id);
    act(() => {
      rowProps(id).onPressIn?.(touch(20));
    });
    expect(background(id)).not.toBe(idleBackground);
    act(() => {
      rowProps(id).onPressOut?.(touch(20));
    });
    expect(background(id)).toBe(idleBackground);
  });
});
