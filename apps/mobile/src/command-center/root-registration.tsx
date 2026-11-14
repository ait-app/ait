import { useMemo } from "react";
import { router, type Href } from "expo-router";
import { useTranslation } from "react-i18next";
import {
  CalendarClock,
  CircleDashed,
  Folder,
  FolderPlus,
  History,
  Home,
  Import,
  Keyboard,
  LayoutDashboard,
  PanelLeft,
  Plus,
  Settings,
  type LucideIcon,
} from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { getIsElectronRuntime, useIsCompactFormFactor } from "@/constants/layout";
import { useKeyboardShortcutOverrides } from "@/hooks/use-keyboard-shortcut-overrides";
import { useOpenAddProject } from "@/hooks/use-open-add-project";
import { useImportSession } from "@/components/use-import-session";
import { useKeyboardActionDispatcher } from "@/keyboard/keyboard-action-dispatcher-context";
import { useKeyboardShortcutsAvailable } from "@/keyboard/availability";
import { resolveShortcutKeysForAction } from "@/keyboard/keyboard-shortcuts";
import { useKeyboardShortcutsStore } from "@/stores/keyboard-shortcuts-store";
import { useToggleAgentList } from "@/components/use-toggle-agent-list";
import { useSidebarViewStore } from "@/stores/sidebar-view-store";
import { clearCommandCenterFocusRestoreElement } from "@/utils/command-center-focus-restore";
import {
  buildOpenProjectRoute,
  buildSchedulesRoute,
  buildTasksRoute,
  buildSessionsRoute,
  buildSettingsRoute,
} from "@/utils/host-routes";
import { getShortcutOs } from "@/utils/shortcut-platform";
import type {
  CommandCenterContribution,
  CommandCenterIcon,
  CommandCenterIconProps,
} from "./contributions";
import { useCommandCenterActions } from "./provider";
import { buildGroupingContribution } from "./root-contributions";

/** A muted lucide glyph at the given stroke, as a Command Center row icon. */
function strokedIcon(Icon: LucideIcon, strokeWidth: number): CommandCenterIcon {
  const ThemedIcon = withUnistyles(Icon, (theme) => ({ color: theme.colors.foregroundMuted }));
  return function StrokedCommandCenterIcon({ size }: CommandCenterIconProps) {
    return <ThemedIcon size={size} strokeWidth={strokeWidth} />;
  };
}

const PlusIcon = strokedIcon(Plus, 2.4);
const AddProjectIcon = strokedIcon(FolderPlus, 2.2);
const SettingsIcon = strokedIcon(Settings, 2.2);
const HistoryIcon = strokedIcon(History, 2.2);
const SchedulesIcon = strokedIcon(CalendarClock, 2.2);
const TasksIcon = strokedIcon(LayoutDashboard, 2.2);
const KeyboardIcon = strokedIcon(Keyboard, 2.2);
const HomeIcon = strokedIcon(Home, 2.2);
const ImportIcon = strokedIcon(Import, 2.2);
const FolderIcon = strokedIcon(Folder, 2.2);
const CircleDashedIcon = strokedIcon(CircleDashed, 2.2);
const PanelLeftIcon = strokedIcon(PanelLeft, 2.2);

export function CommandCenterRootActions() {
  const keyboardActionDispatcher = useKeyboardActionDispatcher();
  const { t } = useTranslation();
  const { overrides } = useKeyboardShortcutOverrides();
  const shortcutsAvailable = useKeyboardShortcutsAvailable();
  const openAddProject = useOpenAddProject();
  const { open: openImportSession, sheet: importSessionSheet } = useImportSession();
  const settingsRoute = useMemo<Href>(() => buildSettingsRoute(), []);
  const homeRoute = useMemo<Href>(() => buildOpenProjectRoute(), []);
  const sessionsRoute = useMemo<Href>(() => buildSessionsRoute(), []);
  const schedulesRoute = useMemo<Href>(() => buildSchedulesRoute(), []);
  const setShortcutsDialogOpen = useKeyboardShortcutsStore((state) => state.setShortcutsDialogOpen);
  // Narrow selector on purpose: a whole-store subscription would re-register every root action
  // each time host filters are reconciled.
  const groupMode = useSidebarViewStore((state) => state.groupMode);
  const setGroupMode = useSidebarViewStore((state) => state.setGroupMode);
  const isCompact = useIsCompactFormFactor();
  const toggleAgentList = useToggleAgentList(isCompact);
  const shortcutPlatform = useMemo(
    () => ({ isMac: getShortcutOs() === "mac", isDesktop: getIsElectronRuntime() }),
    [],
  );
  const actions = useMemo<CommandCenterContribution[]>(() => {
    const availableActions: CommandCenterContribution[] = [
      {
        id: "add-project",
        group: "actions",
        groupRank: 0,
        rank: 0,
        keywords: ["open", "project", "folder", "workspace", "repo"],
        visibility: "query",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          openAddProject();
        },
        presentation: {
          kind: "action",
          title: t("shell.commandCenter.addProject"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: AddProjectIcon,
          // The help id remains "new-agent" because user overrides are keyed by its binding id.
          shortcutKeys:
            resolveShortcutKeysForAction("new-agent", overrides, shortcutPlatform) ?? undefined,
        },
      },
      {
        id: "new-workspace",
        group: "actions",
        groupRank: 0,
        rank: 1,
        keywords: ["new", "workspace", "worktree", "branch"],
        visibility: "always",
        run: () => {
          keyboardActionDispatcher.dispatch({ id: "workspace.new", scope: "sidebar" });
        },
        presentation: {
          kind: "action",
          title: t("sidebar.actions.newWorkspace"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: PlusIcon,
          shortcutKeys:
            resolveShortcutKeysForAction("new-workspace", overrides, shortcutPlatform) ?? undefined,
        },
      },
      {
        id: "import-session",
        group: "actions",
        groupRank: 0,
        rank: 2,
        keywords: ["import", "session", "terminal"],
        visibility: "always",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          openImportSession();
        },
        presentation: {
          kind: "action",
          title: t("importSession.title"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: ImportIcon,
        },
      },
      {
        id: "home",
        group: "actions",
        groupRank: 0,
        rank: 3,
        keywords: ["home", "start", "pair", "device", "providers"],
        visibility: "query",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          router.push(homeRoute);
        },
        presentation: {
          kind: "action",
          title: t("shell.commandCenter.home"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: HomeIcon,
        },
      },
      {
        id: "history",
        group: "actions",
        groupRank: 0,
        rank: 4,
        keywords: ["history", "sessions", "recent"],
        visibility: "always",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          router.push(sessionsRoute);
        },
        presentation: {
          kind: "action",
          title: t("sidebar.sections.sessions"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: HistoryIcon,
        },
      },
      {
        id: "tasks",
        group: "actions",
        groupRank: 0,
        rank: 4.5,
        keywords: ["tasks", "board", "agents", "mission control", "任务", "看板"],
        visibility: "always",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          router.push(buildTasksRoute());
        },
        presentation: {
          kind: "action",
          title: t("taskBoard.title"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: TasksIcon,
        },
      },
      {
        id: "schedules",
        group: "actions",
        groupRank: 0,
        rank: 5,
        keywords: ["schedules", "scheduled", "automation", "recurring"],
        visibility: "always",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          router.push(schedulesRoute);
        },
        presentation: {
          kind: "action",
          title: t("sidebar.sections.schedules"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: SchedulesIcon,
        },
      },
      {
        id: "settings",
        group: "actions",
        groupRank: 0,
        rank: 6,
        keywords: ["settings", "preferences", "config", "configuration"],
        visibility: "always",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          router.push(settingsRoute);
        },
        presentation: {
          kind: "action",
          title: t("sidebar.actions.settings"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: SettingsIcon,
          shortcutKeys:
            resolveShortcutKeysForAction("toggle-settings", overrides, shortcutPlatform) ??
            undefined,
        },
      },
      // Toggle left sidebar is global: it calls the panel store directly and works on every route.
      // The right sidebar and focus toggles do NOT belong here — their handlers live in
      // workspace-screen.tsx behind `enabled: isRouteFocused && ...`, so registering them globally
      // would list entries that silently no-op off a workspace route. They live in
      // workspace-contributions.ts instead. That is why the three toggles render in two
      // non-adjacent sections; don't "tidy" them back together.
      {
        id: "toggle-left-sidebar",
        group: "actions",
        groupRank: 0,
        rank: 8,
        keywords: ["toggle", "sidebar", "left", "panel", "workspaces"],
        visibility: "query",
        run: () => {
          clearCommandCenterFocusRestoreElement();
          toggleAgentList();
        },
        presentation: {
          kind: "action",
          title: t("settings.shortcuts.help.toggleLeftSidebar"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: PanelLeftIcon,
          shortcutKeys:
            resolveShortcutKeysForAction("toggle-left-sidebar", overrides, shortcutPlatform) ??
            undefined,
        },
      },
    ];

    if (shortcutsAvailable) {
      availableActions.push({
        id: "keyboard-shortcuts",
        group: "actions",
        groupRank: 0,
        rank: 7,
        keywords: ["keyboard", "shortcuts", "keys", "hotkeys"],
        visibility: "always",
        run: () => setShortcutsDialogOpen(true),
        presentation: {
          kind: "action",
          title: t("sidebar.help.shortcuts"),
          sectionTitle: t("shell.commandCenter.actions"),
          icon: KeyboardIcon,
          shortcutKeys:
            resolveShortcutKeysForAction("show-shortcuts", overrides, shortcutPlatform) ??
            undefined,
        },
      });
    }

    availableActions.push(
      buildGroupingContribution({
        groupMode,
        labels: {
          section: t("shell.commandCenter.actions"),
          groupByProject: t("shell.commandCenter.groupByProject"),
          groupByStatus: t("shell.commandCenter.groupByStatus"),
        },
        icons: { project: FolderIcon, status: CircleDashedIcon },
        setGroupMode,
      }),
    );

    return availableActions;
  }, [
    groupMode,
    homeRoute,
    keyboardActionDispatcher,
    openAddProject,
    openImportSession,
    overrides,
    schedulesRoute,
    sessionsRoute,
    setGroupMode,
    setShortcutsDialogOpen,
    settingsRoute,
    shortcutPlatform,
    shortcutsAvailable,
    t,
    toggleAgentList,
  ]);

  useCommandCenterActions({ sourceId: "root", enabled: true, actions });
  return importSessionSheet;
}
