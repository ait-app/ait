import { useAppSettings } from "@/hooks/use-settings";
import { useDaemonConfig } from "@/hooks/use-daemon-config";
import { panelCanLaunchInPane, panelSupportsHost, type PaneHost } from "@/panels/panel-manifest";
import type { PanelIconProps } from "@/panels/panel-registry";
import {
  botPanelPresentation,
  changesTreePanelPresentation,
  filesPanelPresentation,
  pullRequestPanelPresentation,
  schedulePanelPresentation,
  workingDiffPanelPresentation,
} from "@/panels/panel-presentations";
import { buildSettingsHostSectionRoute } from "@/utils/host-routes";
import type { WorkspaceTabTarget } from "@/workspace-tabs/model";
import type { NewTabSelection } from "@/workspace-tabs/new-tab";
import type { TerminalProfile } from "@ait/protocol/messages";
import { getTerminalProfileIcon, resolveTerminalProfiles } from "@ait/protocol/terminal-profiles";
import { useRouter, type Href } from "expo-router";
import { Globe, SquarePen, SquareTerminal } from "lucide-react-native";
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  type ComponentType,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import invariant from "tiny-invariant";
import { getBuiltInLaunchOrder, type BuiltInLaunchItemId } from "./internal/catalog";

export type WorkspaceTabLaunchPurpose = "primary" | "supporting";

export type WorkspaceTabLaunchDestination =
  | { kind: "open"; paneId?: string }
  | { kind: "replace"; tabId: string };

export interface NewTabLauncher {
  showChanges: boolean;
  showPullRequest: boolean;
  showBrowser: boolean;
  terminalDisabled: boolean;
  launch: (selection: NewTabSelection, destination: WorkspaceTabLaunchDestination) => void;
}

export interface WorkspaceTabLaunchItem {
  id: string;
  label: string;
  Icon?: ComponentType<PanelIconProps>;
  terminalIconKey?: string;
  shortcutActionId?: string;
  disabled: boolean;
  panelKind: WorkspaceTabTarget["kind"];
  launch: (destination: WorkspaceTabLaunchDestination) => void;
}

interface WorkspaceTabLaunchGroup {
  id: "tabs" | "terminal-profiles";
  label: string | null;
  items: readonly WorkspaceTabLaunchItem[];
  accessory?: { id: string; label: string; run: () => void };
}

const EMPTY_PANE_PANEL_KINDS: readonly WorkspaceTabTarget["kind"][] = [];

const NewTabLauncherContext = createContext<NewTabLauncher | null>(null);

export function NewTabLauncherProvider({
  value,
  children,
}: {
  value: NewTabLauncher;
  children: ReactNode;
}) {
  return <NewTabLauncherContext.Provider value={value}>{children}</NewTabLauncherContext.Provider>;
}

const BUILT_IN_SELECTIONS = {
  agent: { kind: "agent" },
  bot: { kind: "target", target: { kind: "bot" } },
  terminal: { kind: "terminal" },
  changes: { kind: "target", target: { kind: "changes_tree" } },
  diff: { kind: "target", target: { kind: "working_diff" } },
  files: { kind: "target", target: { kind: "files" } },
  browser: { kind: "browser" },
  pullRequest: { kind: "target", target: { kind: "pull_request" } },
  schedule: { kind: "target", target: { kind: "schedule" } },
} satisfies Record<BuiltInLaunchItemId, NewTabSelection>;

export function useWorkspaceTabLaunchCatalog(input: {
  serverId: string;
  purpose: WorkspaceTabLaunchPurpose;
  host: PaneHost;
  surface: "menu" | "panel";
  panePanelKinds?: readonly WorkspaceTabTarget["kind"][];
}): readonly WorkspaceTabLaunchGroup[] {
  const { serverId, purpose, host, surface, panePanelKinds = EMPTY_PANE_PANEL_KINDS } = input;
  const { t } = useTranslation();
  const {
    settings: { experimentalFeatures },
  } = useAppSettings();
  const router = useRouter();
  const launcher = useContext(NewTabLauncherContext);
  invariant(launcher, "NewTabLauncherProvider is required");
  const { config } = useDaemonConfig(serverId);

  const launchSelection = useCallback(
    (selection: NewTabSelection) => (destination: WorkspaceTabLaunchDestination) => {
      launcher.launch(selection, destination);
    },
    [launcher],
  );
  const editTerminalProfiles = useCallback(() => {
    router.push(buildSettingsHostSectionRoute(serverId, "terminals") as Href);
  }, [router, serverId]);

  return useMemo(() => {
    const isExplorerMenu = host === "explorer" && surface === "menu";
    const builtIns: Record<BuiltInLaunchItemId, WorkspaceTabLaunchItem & { hidden?: boolean }> = {
      agent: {
        id: "agent",
        label: t("workspace.tabs.fallback.agent"),
        Icon: SquarePen,
        shortcutActionId: "workspace-tab-target-agent",
        disabled: false,
        panelKind: "draft",
        hidden: isExplorerMenu,
        launch: launchSelection(BUILT_IN_SELECTIONS.agent),
      },
      bot: {
        id: "bot",
        label: "Bot",
        Icon: botPanelPresentation.icon,
        disabled: false,
        hidden: !experimentalFeatures,
        panelKind: "bot",
        launch: launchSelection(BUILT_IN_SELECTIONS.bot),
      },
      terminal: {
        id: "terminal",
        label: t("workspace.tabs.fallback.terminal"),
        Icon: SquareTerminal,
        shortcutActionId: "workspace-terminal-new",
        disabled: launcher.terminalDisabled,
        panelKind: "terminal",
        launch: launchSelection(BUILT_IN_SELECTIONS.terminal),
      },
      changes: {
        id: "changes",
        label: changesTreePanelPresentation.label(t),
        Icon: changesTreePanelPresentation.icon,
        disabled: false,
        panelKind: "changes_tree",
        hidden: !launcher.showChanges,
        launch: launchSelection(BUILT_IN_SELECTIONS.changes),
      },
      diff: {
        id: "diff",
        label: workingDiffPanelPresentation.label(t),
        Icon: workingDiffPanelPresentation.icon,
        shortcutActionId: "workspace-tab-target-changes",
        disabled: false,
        panelKind: "working_diff",
        hidden: !launcher.showChanges,
        launch: launchSelection(BUILT_IN_SELECTIONS.diff),
      },
      files: {
        id: "files",
        label: filesPanelPresentation.label(t),
        Icon: filesPanelPresentation.icon,
        shortcutActionId: "workspace-tab-target-files",
        disabled: false,
        panelKind: "files",
        launch: launchSelection(BUILT_IN_SELECTIONS.files),
      },
      browser: {
        id: "browser",
        label: t("workspace.tabs.fallback.browser"),
        Icon: Globe,
        shortcutActionId: "workspace-tab-target-browser",
        disabled: false,
        panelKind: "browser",
        hidden: !launcher.showBrowser,
        launch: launchSelection(BUILT_IN_SELECTIONS.browser),
      },
      pullRequest: {
        id: "pull-request",
        label: pullRequestPanelPresentation.label(t),
        Icon: pullRequestPanelPresentation.icon,
        disabled: false,
        panelKind: "pull_request",
        hidden: !launcher.showPullRequest,
        launch: launchSelection(BUILT_IN_SELECTIONS.pullRequest),
      },
      schedule: {
        id: "schedule",
        label: schedulePanelPresentation.label(t),
        Icon: schedulePanelPresentation.icon,
        disabled: false,
        panelKind: "schedule",
        launch: launchSelection(BUILT_IN_SELECTIONS.schedule),
      },
    };
    const tabItems = getBuiltInLaunchOrder(purpose).flatMap((id) => {
      const item = builtIns[id];
      return item.hidden || !panelSupportsHost(item.panelKind, host) ? [] : [item];
    });

    const profiles = resolveTerminalProfiles(config?.terminalProfiles);
    const groups: WorkspaceTabLaunchGroup[] = [{ id: "tabs", label: null, items: tabItems }];
    if (!isExplorerMenu && profiles.length > 0) {
      groups.push({
        id: "terminal-profiles",
        label: t("workspace.tabs.actions.terminalProfilesMenu"),
        items: profiles.map((profile: TerminalProfile) => ({
          id: `terminal-profile:${profile.id}`,
          label: profile.name,
          terminalIconKey: getTerminalProfileIcon(profile),
          disabled: launcher.terminalDisabled,
          panelKind: "terminal",
          launch: launchSelection({ kind: "terminal", profile }),
        })),
        accessory: {
          id: "edit-terminal-profiles",
          label: t("workspace.tabs.actions.editTerminalProfiles"),
          run: editTerminalProfiles,
        },
      });
    }
    if (surface !== "menu") return groups;
    return groups.flatMap((group) => {
      const items = group.items.filter((item) =>
        panelCanLaunchInPane(item.panelKind, panePanelKinds),
      );
      return items.length > 0 ? [{ ...group, items }] : [];
    });
  }, [
    experimentalFeatures,
    config?.terminalProfiles,
    editTerminalProfiles,
    launchSelection,
    launcher,
    purpose,
    host,
    surface,
    panePanelKinds,
    t,
  ]);
}
