import { groupHosts } from "@/hosts/host-groups";
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type ReactElement,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import {
  Platform,
  Pressable,
  StatusBar,
  Text,
  View,
  type GestureResponderEvent,
  type PressableStateCallbackType,
} from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import {
  Captions,
  Circle,
  CircleCheck,
  CircleDashed,
  Clock,
  Diff,
  EyeOff,
  Folder,
  GitBranch,
  GitPullRequest,
  Globe,
  Server,
  Settings2,
  Tag,
  Type,
} from "lucide-react-native";
import {
  MenuItem,
  MenuRoot,
  MenuSeparator,
  MenuSubTrigger,
  MenuSurface,
  useMenuContext,
  type MenuPageDefinition,
} from "@/components/ui/menu";
import { measureElement } from "@/components/ui/menu/menu-anchor";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { foregroundIconColorMapping, mutedIconColorMapping } from "@/components/ui/icon-color";
import { HostStatusDot } from "@/components/host-status-dot";
import { isWeb } from "@/constants/platform";
import { useHosts } from "@/runtime/host-runtime";
import { useSidebarModel } from "@/components/sidebar/sidebar-model";
import { ProjectIconView } from "@/components/project-icon-view";
import { useProjectIcons } from "@/projects/icons";
import { resolveSidebarProjectIconTargets } from "@/utils/sidebar-project-row-model";
import { displayInitial } from "@/utils/display-initial";
import { projectIconPlaceholderLabelFromDisplayName } from "@/utils/project-display-name";
import type { SidebarProjectEntry } from "@/hooks/use-sidebar-workspaces-list";
import type { Theme } from "@/styles/theme";
import {
  hasActiveSidebarLabelFilter,
  SIDEBAR_UNLABELLED_LABEL_KEY,
  type SidebarGroupMode,
} from "@/stores/sidebar-view-store";
import { workspaceLabelKey, type WorkspaceLabelColor } from "@ait/protocol/workspace-labels";
import type { WorkspaceTitleSource } from "@/hooks/use-settings";
import { SIDEBAR_CHECKS_DISPLAYS, type SidebarChecksDisplay } from "./checks-display";
import { useSidebarDisplayPreferences, type SidebarTrailingChoice } from "./model";
import { SIDEBAR_ROW_ITEMS, type SidebarRowItem } from "./row-items";
import { useWorkspaceLabelProjection } from "@/workspace-labels";
import { WorkspaceLabelDot } from "@/workspace-labels/swatch";
import { WorkspaceLabelManagerModal } from "@/workspace-labels/manager-modal";

const ThemedSettings2 = withUnistyles(Settings2);
/** CI's mark: the subject of the checks row, and the shape the icon-only option leaves behind. */
const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircle = withUnistyles(Circle);

/** Fits the item's 16pt leading slot with a hair of room, matching the trailing check. */
const OPTION_ICON_SIZE = 14;
const MENU_WIDTH = 232;

/**
 * Unlabelled's stand-in for a color dot: the same circle at the same size, hollow.
 *
 * 11 rather than the dot's 10 because lucide draws a `size` box and puts a stroked r=10-of-24
 * circle inside it, so the ring's outer edge lands on the dots' edge at 11 and 1pt short at 10.
 * The glyphs are what have to agree here, not the boxes they are centred in.
 */
const UNLABELLED_MARK = <ThemedCircle size={11} uniProps={mutedIconColorMapping} />;

type OptionIcon = ComponentType<{
  size: number;
  uniProps: (theme: Theme) => { color: string };
}>;

// Options carry icons; the root rows deliberately do not. The root is four labels with their
// current values, and a column of icons there would be decoration competing with the values.
const GROUPING_ICONS: Record<SidebarGroupMode, OptionIcon> = {
  project: withUnistyles(Folder),
  status: withUnistyles(CircleDashed),
};

const TITLE_SOURCE_ICONS: Record<WorkspaceTitleSource, OptionIcon> = {
  title: withUnistyles(Type),
  branch: withUnistyles(GitBranch),
};

// The same marks these things carry on the workspace row itself, so the menu and the row it
// configures name each item the same way twice.
const ROW_ITEM_ICONS: Record<SidebarRowItem, OptionIcon> = {
  branch: withUnistyles(GitBranch),
  project: withUnistyles(Folder),
  host: withUnistyles(Server),
  changeRequest: withUnistyles(GitPullRequest),
  services: withUnistyles(Globe),
  labels: withUnistyles(Tag),
};

// These mark how much of the row an option spends, not what CI is, so they are the shapes each
// answer produces: a glyph with words beside it, the glyph on its own, nothing.
const CHECKS_DISPLAY_ICONS: Record<SidebarChecksDisplay, OptionIcon> = {
  iconAndText: withUnistyles(Captions),
  icon: ThemedCircleCheck,
  none: withUnistyles(EyeOff),
};

const TRAILING_ICONS: Record<SidebarTrailingChoice, OptionIcon> = {
  diff: withUnistyles(Diff),
  timestamp: withUnistyles(Clock),
};

const GROUPING_MODES: readonly SidebarGroupMode[] = ["project", "status"];
const TITLE_SOURCES: readonly WorkspaceTitleSource[] = ["title", "branch"];
const TRAILING_CHOICES: readonly SidebarTrailingChoice[] = ["diff", "timestamp"];

const GROUPING_LABEL_KEYS: Record<SidebarGroupMode, string> = {
  project: "sidebar.display.grouping.project",
  status: "sidebar.display.grouping.status",
};

const TITLE_SOURCE_LABEL_KEYS: Record<WorkspaceTitleSource, string> = {
  title: "sidebar.display.titleSource.title",
  branch: "sidebar.display.titleSource.branch",
};

const ROW_ITEM_LABEL_KEYS: Record<SidebarRowItem, string> = {
  branch: "sidebar.display.show.branch",
  project: "sidebar.display.show.project",
  host: "sidebar.display.show.host",
  changeRequest: "sidebar.display.show.changeRequest",
  services: "sidebar.display.show.services",
  labels: "sidebar.display.show.labels",
};

const CHECKS_DISPLAY_LABEL_KEYS: Record<SidebarChecksDisplay, string> = {
  iconAndText: "sidebar.display.checks.iconAndText",
  icon: "sidebar.display.checks.icon",
  none: "sidebar.display.checks.none",
};

const TRAILING_LABEL_KEYS: Record<SidebarTrailingChoice, string> = {
  diff: "sidebar.display.show.diff",
  timestamp: "sidebar.display.show.timestamp",
};

interface DisplayMenuHost {
  /** The trigger the menu is open from, or null while it is closed. */
  activeTriggerId: string | null;
  toggle: (triggerId: string, trigger: View | null) => void;
}

const DisplayMenuHostContext = createContext<DisplayMenuHost | null>(null);

/**
 * The sidebar's one display menu, opened from a trigger on each row that heads a group.
 *
 * The menu belongs to the list rather than to any row because its own choices take rows away:
 * filtering to another project, or switching the grouping, unmounts the row it was opened from.
 * A menu living in that row would close under the pointer halfway through a selection. Here the
 * triggers only say where to anchor, and the surface outlives them.
 */
export function SidebarDisplayPreferencesMenuHost({
  children,
}: {
  children: ReactNode;
}): ReactElement {
  const [activeTriggerId, setActiveTriggerId] = useState<string | null>(null);
  const handleOpenChange = useCallback((open: boolean) => {
    if (!open) setActiveTriggerId(null);
  }, []);
  // Held here rather than in the surface: choosing "Manage labels" closes the menu, and the
  // surface, which only exists while the menu is open, goes with it. The manager mounts on its
  // first use and then stays, so closing it is its own transition rather than an unmount.
  const [manager, setManager] = useState<"unused" | "open" | "closed">("unused");
  const openManager = useCallback(() => setManager("open"), []);
  const closeManager = useCallback(() => setManager("closed"), []);

  return (
    <>
      <MenuRoot compactMode="sheet" open={activeTriggerId !== null} onOpenChange={handleOpenChange}>
        <DisplayMenuTriggers activeTriggerId={activeTriggerId} onOpen={setActiveTriggerId}>
          {children}
        </DisplayMenuTriggers>
        {/* Everything the menu reads is subscribed to only while it is open. */}
        {activeTriggerId !== null ? (
          <SidebarDisplayPreferencesSurface onManageLabels={openManager} />
        ) : null}
      </MenuRoot>
      {manager === "unused" ? null : (
        <WorkspaceLabelManagerModal visible={manager === "open"} onClose={closeManager} />
      )}
    </>
  );
}

function DisplayMenuTriggers({
  activeTriggerId,
  onOpen,
  children,
}: {
  activeTriggerId: string | null;
  onOpen: (triggerId: string) => void;
  children: ReactNode;
}): ReactElement {
  const menu = useMenuContext("SidebarDisplayPreferencesMenuHost");
  const { setAnchorRect, setOpen, triggerRef } = menu;
  const isOpen = activeTriggerId !== null;

  const toggle = useCallback(
    (triggerId: string, trigger: View | null) => {
      if (isOpen) {
        setOpen(false);
        return;
      }
      // Focus returns here on close, when the trigger is still on screen.
      Object.assign(triggerRef, { current: trigger });
      if (!trigger) {
        onOpen(triggerId);
        return;
      }
      // Read synchronously so the offset cannot change before the measurement resolves. The
      // surface is drawn from the screen top, and Android measures from below the status bar.
      const statusBarHeight = Platform.OS === "android" ? (StatusBar.currentHeight ?? 0) : 0;
      void measureElement(trigger).then((rect) => {
        // A fixed box rather than the trigger itself: the row may be gone by the next layout.
        setAnchorRect({ ...rect, y: rect.y + statusBarHeight });
        onOpen(triggerId);
        return undefined;
      });
    },
    [isOpen, onOpen, setAnchorRect, setOpen, triggerRef],
  );

  const value = useMemo<DisplayMenuHost>(
    () => ({ activeTriggerId, toggle }),
    [activeTriggerId, toggle],
  );
  return (
    <DisplayMenuHostContext.Provider value={value}>{children}</DisplayMenuHostContext.Provider>
  );
}

/**
 * A row's button for the display menu. `visible` is the row's own reveal rule (hover on desktop,
 * always on touch); the trigger also stays in place for as long as the menu is open from it, so
 * the menu is never left pointing at an empty slot.
 */
export function SidebarDisplayPreferencesTrigger({
  triggerId,
  visible = true,
}: {
  triggerId: string;
  visible?: boolean;
}): ReactElement {
  const { t } = useTranslation();
  const host = useContext(DisplayMenuHostContext);
  if (!host) {
    throw new Error(
      "SidebarDisplayPreferencesTrigger must be used within SidebarDisplayPreferencesMenuHost",
    );
  }
  const { activeTriggerId, toggle } = host;
  const triggerRef = useRef<View>(null);
  const active = activeTriggerId === triggerId;
  const shown = visible || active;
  const label = t("sidebar.display.trigger");

  const handlePress = useCallback(
    (event: GestureResponderEvent) => {
      // The row underneath collapses its group on press.
      event.stopPropagation();
      toggle(triggerId, triggerRef.current);
    },
    [toggle, triggerId],
  );
  const triggerStyle = useCallback(
    ({ hovered = false, pressed }: PressableStateCallbackType & { hovered?: boolean }) => [
      styles.trigger,
      !shown && styles.triggerHidden,
      (hovered || pressed || active) && styles.triggerHovered,
    ],
    [active, shown],
  );
  const renderIcon = useCallback(
    ({ hovered = false, pressed }: PressableStateCallbackType & { hovered?: boolean }) => (
      <ThemedSettings2
        size={14}
        uniProps={hovered || pressed || active ? foregroundIconColorMapping : mutedIconColorMapping}
      />
    ),
    [active],
  );
  const accessibilityState = useMemo(() => ({ expanded: active }), [active]);
  const webExpandedState = useMemo(
    () => (isWeb ? ({ "aria-expanded": active } as const) : null),
    [active],
  );

  return (
    <View style={styles.triggerSlot} pointerEvents={shown ? "auto" : "none"}>
      <Tooltip delayDuration={300} enabledOnDesktop enabledOnMobile={false}>
        <TooltipTrigger asChild disabled={!shown || active}>
          <Pressable
            {...webExpandedState}
            ref={triggerRef}
            collapsable={false}
            style={triggerStyle}
            onPress={handlePress}
            accessibilityRole={isWeb ? undefined : "button"}
            accessibilityLabel={label}
            accessibilityState={accessibilityState}
            testID={`sidebar-display-preferences-menu-${triggerId}`}
          >
            {renderIcon}
          </Pressable>
        </TooltipTrigger>
        <TooltipContent side="bottom" align="center" offset={8}>
          <Text style={styles.tooltipText}>{label}</Text>
        </TooltipContent>
      </Tooltip>
    </View>
  );
}

/**
 * The display menu's trigger for a list with no row to carry one.
 *
 * A filter can leave the sidebar without a single group heading, and the menu is the only way
 * back out of most filters, so a list in that state renders this in the headings' place.
 */
export function SidebarDisplayPreferencesFallbackRow(): ReactElement {
  return (
    <View style={styles.fallbackRow} testID="sidebar-display-preferences-fallback">
      <SidebarDisplayPreferencesTrigger triggerId="fallback" />
    </View>
  );
}

/**
 * What the sidebar shows and how it is arranged.
 *
 * The root is one row per decision with its current value; the options live a level down. The
 * shape is deliberate — every option of every decision on one surface is what this menu used to
 * be, and it grew a row for each host on top of that.
 */
function SidebarDisplayPreferencesSurface({
  onManageLabels,
}: {
  onManageLabels: () => void;
}): ReactElement {
  const { t } = useTranslation();
  const preferences = useSidebarDisplayPreferences();
  const hosts = useHosts();
  // `allProjects`, never `projects`: the model's `projects` is already filtered, so a picker fed
  // from it would lose the row that undoes the filter as soon as the filter narrowed to one.
  const { allProjects, resolvedProjectFilters } = useSidebarModel();
  const { labels } = useWorkspaceLabelProjection();

  const showHostFilter = groupHosts(hosts).length > 1;
  // One project is the whole sidebar, so filtering to it is a no-op with a menu row attached.
  const showProjectFilter = allProjects.length > 1;
  // Nothing to filter by means no row at all. The active-filter half is not redundant: the merged
  // catalog only counts hosts that are online, so a host dropping off would otherwise take away
  // the only way back to a filter that is still hiding workspaces.
  const showLabelFilter = labels.length > 0 || hasActiveSidebarLabelFilter(preferences.labelFilter);

  const pages = useMemo<MenuPageDefinition[]>(() => {
    const definitions: MenuPageDefinition[] = [
      {
        id: "grouping",
        title: t("sidebar.display.grouping.label"),
        content: (
          <OptionList
            values={GROUPING_MODES}
            icons={GROUPING_ICONS}
            labelKeys={GROUPING_LABEL_KEYS}
            selectedValue={preferences.grouping}
            onSelect={preferences.setGrouping}
            testIDPrefix="sidebar-grouping"
          />
        ),
      },
      {
        id: "titleSource",
        title: t("sidebar.display.titleSource.label"),
        content: (
          <OptionList
            values={TITLE_SOURCES}
            icons={TITLE_SOURCE_ICONS}
            labelKeys={TITLE_SOURCE_LABEL_KEYS}
            selectedValue={preferences.titleSource}
            onSelect={preferences.setTitleSource}
            testIDPrefix="sidebar-workspace-title-source"
          />
        ),
      },
      {
        id: "show",
        title: t("sidebar.display.show.label"),
        content: <ShowPage preferences={preferences} />,
      },
      {
        id: "checks",
        title: t("sidebar.display.show.checks"),
        content: (
          <OptionList
            values={SIDEBAR_CHECKS_DISPLAYS}
            icons={CHECKS_DISPLAY_ICONS}
            labelKeys={CHECKS_DISPLAY_LABEL_KEYS}
            selectedValue={preferences.checksDisplay}
            onSelect={preferences.setChecksDisplay}
            testIDPrefix="sidebar-checks-display"
          />
        ),
      },
    ];

    if (showHostFilter) {
      definitions.push({
        id: "hostFilter",
        title: t("sidebar.display.hostFilter.label"),
        content: <HostFilterPage preferences={preferences} hosts={hosts} />,
      });
    }
    if (showProjectFilter) {
      definitions.push({
        id: "projectFilter",
        title: t("sidebar.display.projectFilter.label"),
        content: (
          <ProjectFilterPage
            projects={allProjects}
            resolvedProjectFilters={resolvedProjectFilters}
            preferences={preferences}
          />
        ),
      });
    }
    if (showLabelFilter) {
      definitions.push({
        id: "labelFilter",
        title: t("workspaceLabels.title"),
        content: (
          <LabelFilterPage labels={labels} preferences={preferences} onManage={onManageLabels} />
        ),
      });
    }
    return definitions;
  }, [
    t,
    preferences,
    hosts,
    showHostFilter,
    showProjectFilter,
    allProjects,
    resolvedProjectFilters,
    showLabelFilter,
    labels,
    onManageLabels,
  ]);

  return (
    <MenuSurface
      align="end"
      width={MENU_WIDTH}
      pages={pages}
      sheetTitle={t("sidebar.display.heading")}
      testID="sidebar-display-preferences-content"
    >
      <MenuSubTrigger
        id="grouping"
        value={t(GROUPING_LABEL_KEYS[preferences.grouping])}
        testID="sidebar-display-grouping"
      >
        {t("sidebar.display.grouping.label")}
      </MenuSubTrigger>
      <MenuSubTrigger
        id="titleSource"
        value={t(TITLE_SOURCE_LABEL_KEYS[preferences.titleSource])}
        testID="sidebar-display-title-source"
      >
        {t("sidebar.display.titleSource.label")}
      </MenuSubTrigger>
      <MenuSubTrigger id="show" testID="sidebar-display-show">
        {t("sidebar.display.show.label")}
      </MenuSubTrigger>
      {showHostFilter ? (
        <>
          <MenuSeparator />
          {/* A filtered sidebar looks like workspaces went missing, so the branch says so
            from the root rather than making you open it to find out. */}
          <MenuSubTrigger
            id="hostFilter"
            indicator={preferences.hostFilters.length > 0}
            testID="sidebar-display-host-filter"
          >
            {t("sidebar.display.hostFilter.label")}
          </MenuSubTrigger>
        </>
      ) : null}
      {showProjectFilter ? (
        <>
          {/* Host and Project narrow the same list, so they read as one block. The separator
            belongs above whichever of the two is showing first — with a single host there is
            no Host row and Project is what has to carry it. */}
          {showHostFilter ? null : <MenuSeparator />}
          <MenuSubTrigger
            id="projectFilter"
            indicator={resolvedProjectFilters.length > 0}
            testID="sidebar-display-project-filter"
          >
            {t("sidebar.display.projectFilter.label")}
          </MenuSubTrigger>
        </>
      ) : null}
      {showLabelFilter ? (
        <>
          <MenuSeparator />
          <MenuSubTrigger
            id="labelFilter"
            indicator={hasActiveSidebarLabelFilter(preferences.labelFilter)}
            testID="sidebar-display-label-filter"
          >
            {t("workspaceLabels.title")}
          </MenuSubTrigger>
        </>
      ) : null}
    </MenuSurface>
  );
}

/**
 * Every label you could filter by, wherever it lives, one row each.
 *
 * The catalog is the merged cross-host one on purpose: a workspace row draws its label in its own
 * host's color because it would otherwise lie, but this page is the whole set of things to filter
 * on and a per-host split would only make you visit it twice.
 *
 * Clear is absent rather than disabled when it has nothing to act on.
 */
function LabelFilterPage({
  labels,
  preferences,
  onManage,
}: {
  labels: ReturnType<typeof useWorkspaceLabelProjection>["labels"];
  preferences: Preferences;
  onManage: () => void;
}): ReactElement {
  const { t } = useTranslation();
  const selected = preferences.labelFilter.labels;
  return (
    <>
      {labels.map((label) => (
        <LabelFilterItem
          key={workspaceLabelKey(label.name)}
          name={label.name}
          label={label.name}
          color={label.color}
          selected={selected.includes(workspaceLabelKey(label.name))}
          onToggle={preferences.toggleLabelFilter}
          testID={`sidebar-label-filter-option-${label.name}`}
        />
      ))}
      <LabelFilterItem
        name={SIDEBAR_UNLABELLED_LABEL_KEY}
        label={t("workspaceLabels.unlabelled")}
        color={null}
        selected={selected.includes(SIDEBAR_UNLABELLED_LABEL_KEY)}
        onToggle={preferences.toggleLabelFilter}
        testID="sidebar-label-filter-option-unlabelled"
      />
      {hasActiveSidebarLabelFilter(preferences.labelFilter) ? (
        <>
          <MenuSeparator />
          <MenuItem
            closeOnSelect={false}
            onSelect={preferences.clearLabelFilter}
            testID="sidebar-label-filter-clear"
          >
            {t("workspaceLabels.filter.clear")}
          </MenuItem>
        </>
      ) : null}
      <MenuSeparator />
      <MenuItem onSelect={onManage} testID="sidebar-label-manage">
        {t("workspaceLabels.manage.open")}
      </MenuItem>
    </>
  );
}

/**
 * One label: its colour leading, its name, and the engine's own check when it is filtered on.
 *
 * The same row the host filter uses, because it is the same question — one press in, one press
 * out, several at once. Four attempts at giving exclusion a shape in this row are in the branch
 * history; each bought a new awkwardness, because two controls in one 232pt menu row is a lot of
 * machinery for a filter nobody asked to invert.
 */
function LabelFilterItem({
  name,
  label,
  color,
  selected,
  onToggle,
  testID,
}: {
  /** The filter key this row acts on. Empty for Unlabelled — see `SIDEBAR_UNLABELLED_LABEL_KEY`. */
  name: string;
  label: string;
  /** `null` is Unlabelled, the one row with no color to stand for. */
  color: WorkspaceLabelColor | null;
  selected: boolean;
  onToggle: (name: string) => void;
  testID: string;
}): ReactElement {
  const handleSelect = useCallback(() => onToggle(name), [name, onToggle]);
  const leading = useMemo(
    () => (color ? <WorkspaceLabelDot color={color} /> : UNLABELLED_MARK),
    [color],
  );

  return (
    <MenuItem
      selected={selected}
      leading={leading}
      closeOnSelect={false}
      onSelect={handleSelect}
      testID={testID}
    >
      {label}
    </MenuItem>
  );
}

type Preferences = ReturnType<typeof useSidebarDisplayPreferences>;

/** One option row: its mark on the left, its label, and a check when it is the chosen one. */
function OptionItem<Value extends string>({
  value,
  icon: Icon,
  label,
  selected,
  closeOnSelect = true,
  onSelect,
  testID,
}: {
  value: Value;
  icon: OptionIcon;
  label: string;
  selected: boolean;
  closeOnSelect?: boolean;
  onSelect: (value: Value) => void;
  testID: string;
}): ReactElement {
  const handleSelect = useCallback(() => onSelect(value), [onSelect, value]);
  const leading = useMemo(
    () => <Icon size={OPTION_ICON_SIZE} uniProps={mutedIconColorMapping} />,
    [Icon],
  );
  return (
    <MenuItem
      selected={selected}
      leading={leading}
      closeOnSelect={closeOnSelect}
      onSelect={handleSelect}
      testID={testID}
    >
      {label}
    </MenuItem>
  );
}

/** A page of mutually exclusive options — pick one and the menu closes. */
function OptionList<Value extends string>({
  values,
  icons,
  labelKeys,
  selectedValue,
  onSelect,
  testIDPrefix,
}: {
  values: readonly Value[];
  icons: Record<Value, OptionIcon>;
  labelKeys: Record<Value, string>;
  selectedValue: Value;
  onSelect: (value: Value) => void;
  testIDPrefix: string;
}): ReactNode {
  const { t } = useTranslation();
  return values.map((value) => (
    <OptionItem
      key={value}
      value={value}
      icon={icons[value]}
      label={t(labelKeys[value])}
      selected={value === selectedValue}
      onSelect={onSelect}
      testID={`${testIDPrefix}-${value}`}
    />
  ));
}

/**
 * Two groups, split by the separator. Above it, what a row may say about a workspace — each one
 * independent. Below it, the one thing the slot to the right of the title holds, so picking the
 * one already showing empties the slot and gives the width back to the title.
 *
 * CI is the one item above the separator with three answers rather than two, so it opens a page
 * instead of ticking, and it goes last: a row that navigates does not belong in the middle of a
 * column you are running down with your eyes ticking things on and off.
 */
function ShowPage({ preferences }: { preferences: Preferences }): ReactElement {
  const { t } = useTranslation();
  return (
    <>
      {SIDEBAR_ROW_ITEMS.map((item) => (
        <OptionItem
          key={item}
          value={item}
          icon={ROW_ITEM_ICONS[item]}
          label={t(ROW_ITEM_LABEL_KEYS[item])}
          selected={preferences.rowItems[item]}
          closeOnSelect={false}
          onSelect={preferences.toggleRowItem}
          testID={`sidebar-row-item-${item}`}
        />
      ))}
      <ChecksSubTrigger />
      <MenuSeparator />
      {TRAILING_CHOICES.map((choice) => (
        <OptionItem
          key={choice}
          value={choice}
          icon={TRAILING_ICONS[choice]}
          label={t(TRAILING_LABEL_KEYS[choice])}
          selected={preferences.trailing === choice}
          closeOnSelect={false}
          onSelect={preferences.toggleTrailing}
          testID={`sidebar-workspace-trailing-${choice}`}
        />
      ))}
    </>
  );
}

/**
 * No value on the row, only the chevron. The three answers are all long enough that the value
 * fills the row and stops reading as an answer sitting at the right edge — it turns into a second
 * line of label. The chevron alone says there is a decision in here, and the page says what it is.
 */
function ChecksSubTrigger(): ReactElement {
  const { t } = useTranslation();
  const leading = useMemo(
    () => <ThemedCircleCheck size={OPTION_ICON_SIZE} uniProps={mutedIconColorMapping} />,
    [],
  );
  return (
    <MenuSubTrigger id="checks" leading={leading} testID="sidebar-display-checks">
      {t("sidebar.display.show.checks")}
    </MenuSubTrigger>
  );
}

/**
 * Every project the sidebar could show, one row each.
 *
 * A workspace belongs to exactly one project, so this is a plain allowlist — the same shape as the
 * host page, and deliberately not the label page's tri-state.
 *
 * Selection reads `resolvedProjectFilters`, not the stored list. A stored key whose project is not
 * currently visible filters nothing, so showing it as checked here would contradict the sidebar.
 */
function ProjectFilterPage({
  projects,
  resolvedProjectFilters,
  preferences,
}: {
  projects: readonly SidebarProjectEntry[];
  resolvedProjectFilters: readonly string[];
  preferences: Preferences;
}): ReactElement {
  const { t } = useTranslation();
  const iconTargets = useMemo(() => resolveSidebarProjectIconTargets(projects), [projects]);
  // Shares TanStack's cache with the sidebar's own call, so this subscribes rather than refetches.
  const iconByProjectViewKey = useProjectIcons({ projects: iconTargets });

  return (
    <>
      <MenuItem
        selected={resolvedProjectFilters.length === 0}
        closeOnSelect={false}
        onSelect={preferences.clearProjectFilters}
        testID="sidebar-project-filter-all"
      >
        {t("sidebar.display.projectFilter.all")}
      </MenuItem>
      {projects.map((project) => (
        <ProjectFilterItem
          key={project.viewKey}
          viewKey={project.viewKey}
          label={project.projectName}
          iconDataUri={iconByProjectViewKey.get(project.viewKey) ?? null}
          selected={resolvedProjectFilters.includes(project.viewKey)}
          onToggle={preferences.toggleProjectFilter}
        />
      ))}
    </>
  );
}

function ProjectFilterItem({
  viewKey,
  label,
  iconDataUri,
  selected,
  onToggle,
}: {
  viewKey: string;
  label: string;
  iconDataUri: string | null;
  selected: boolean;
  onToggle: (viewKey: string) => void;
}): ReactElement {
  const handleSelect = useCallback(() => onToggle(viewKey), [viewKey, onToggle]);
  const leading = useMemo(
    () => (
      <ProjectIconView
        iconDataUri={iconDataUri}
        initial={displayInitial(projectIconPlaceholderLabelFromDisplayName(label))}
        projectViewKey={viewKey}
        size={OPTION_ICON_SIZE}
        textStyle={styles.projectIconText}
      />
    ),
    [iconDataUri, label, viewKey],
  );

  return (
    <MenuItem
      selected={selected}
      leading={leading}
      closeOnSelect={false}
      onSelect={handleSelect}
      testID={`sidebar-project-filter-${viewKey}`}
    >
      {label}
    </MenuItem>
  );
}

function HostFilterPage({
  preferences,
  hosts,
}: {
  preferences: Preferences;
  hosts: ReturnType<typeof useHosts>;
}): ReactElement {
  const { t } = useTranslation();
  return (
    <>
      <MenuItem
        selected={preferences.hostFilters.length === 0}
        closeOnSelect={false}
        onSelect={preferences.clearHostFilters}
        testID="sidebar-host-filter-all"
      >
        {t("sidebar.display.hostFilter.all")}
      </MenuItem>
      {groupHosts(hosts).map((host) => (
        <HostFilterItem
          key={host.serverId}
          serverId={host.serverId}
          label={host.label?.trim() || host.serverId}
          selected={host.memberServerIds.some((id) => preferences.hostFilters.includes(id))}
          onToggle={() => {
            const selected = host.memberServerIds.some((id) =>
              preferences.hostFilters.includes(id),
            );
            for (const id of host.memberServerIds) {
              if (preferences.hostFilters.includes(id) === selected)
                preferences.toggleHostFilter(id);
            }
          }}
        />
      ))}
    </>
  );
}

/** The one option row whose mark is live state rather than an icon. */
function HostFilterItem({
  serverId,
  label,
  selected,
  onToggle,
}: {
  serverId: string;
  label: string;
  selected: boolean;
  onToggle: (serverId: string) => void;
}): ReactElement {
  const handleSelect = useCallback(() => onToggle(serverId), [onToggle, serverId]);
  const leading = useMemo(
    () => (
      <View testID={`sidebar-host-filter-status-${serverId}`}>
        <HostStatusDot serverId={serverId} />
      </View>
    ),
    [serverId],
  );

  return (
    <MenuItem
      selected={selected}
      closeOnSelect={false}
      leading={leading}
      onSelect={handleSelect}
      testID={`sidebar-host-filter-${serverId}`}
    >
      {label}
    </MenuItem>
  );
}

const styles = StyleSheet.create((theme) => ({
  // The 24pt control the rows' other trailing actions use, so a row's buttons read as one set.
  triggerSlot: {
    width: 24,
    height: 24,
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  trigger: {
    width: 24,
    height: 24,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: theme.borderRadius.md,
    flexShrink: 0,
  },
  triggerHidden: {
    opacity: 0,
  },
  triggerHovered: {
    backgroundColor: theme.colors.surface2,
  },
  tooltipText: {
    color: theme.colors.popoverForeground,
    fontSize: theme.fontSize.base,
  },
  fallbackRow: {
    flexDirection: "row",
    justifyContent: "flex-end",
    // Lands the glyph on the rail the rows' trailing actions sit on.
    paddingRight: 2,
    paddingVertical: theme.spacing[1],
  },
  // The icon sits in a 14pt menu slot, so the fallback initial is sized down to match rather
  // than reusing the sidebar row's 16pt figure.
  projectIconText: {
    fontSize: 8,
  },
}));
