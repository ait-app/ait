import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { Fragment, useCallback, useMemo, useState, type ReactElement } from "react";
import { View, Text, Pressable, type PressableStateCallbackType } from "react-native";
import { StyleSheet, useUnistyles, withUnistyles } from "react-native-unistyles";
import { ChevronDown, GitBranch } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuHint,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
  type MenuPageDefinition,
} from "@/components/ui/dropdown-menu";
import { mutedIconColorMapping, statusWarningIconColorMapping } from "@/components/ui/icon-color";
import { inlineUnistylesStyle } from "@/styles/unistyles-inline-style";
import type {
  GitControl,
  GitControlAction,
  GitControlMergeMethod,
  GitControlPrimary,
  GitControlSection,
  GitControlSummary,
} from "@/git/control";
import type { CheckoutPrMergeMethod } from "@ait/protocol/messages";
import { useGitActionRunner } from "@/git/use-actions";
import { GIT_ACTION_ICONS } from "@/git/action-icons";
import { buttonControlHeight, HEADER_CONTROL_HEIGHT } from "@/components/ui/control-geometry";

const MERGE_METHOD_PAGE_ID = "git-merge-method";
const PANEL_MIN_WIDTH = 260;

const ThemedGitBranch = withUnistyles(GitBranch);

interface GitActionsSplitButtonProps {
  control: GitControl;
  /** Only the panel, behind a menu trigger (the Changes panel's Git menu). */
  menuOnly?: boolean;
}

/**
 * The workspace Git control: a main button that runs the next step, or names the state and opens
 * the panel, plus a caret that opens the panel with the status summary and grouped actions.
 */
export function GitActionsSplitButton({ control, menuOnly = false }: GitActionsSplitButtonProps) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const runGitAction = useGitActionRunner();
  const [open, setOpen] = useState(false);
  const pages = useMergeMethodPages(control.mergeMethod);
  const { primary } = control;

  const handlePrimaryPress = useCallback(() => {
    if (primary?.kind === "action") {
      runGitAction(primary.action);
      return;
    }
    setOpen(true);
  }, [primary, runGitAction]);

  const primaryDisabled = primary?.kind === "action" && primary.action.disabled;
  const primaryPressableStyle = useCallback(
    ({ hovered, pressed }: PressableStateCallbackType & { hovered?: boolean }) => [
      styles.splitButtonPrimary,
      (Boolean(hovered) || pressed) &&
        inlineUnistylesStyle({ backgroundColor: theme.colors.surface2 }),
      primaryDisabled && styles.splitButtonPrimaryDisabled,
    ],
    [primaryDisabled, theme.colors.surface2],
  );

  const caretTriggerStyle = useCallback(
    ({ hovered, pressed, open: isOpen }: { hovered: boolean; pressed: boolean; open: boolean }) => [
      styles.splitButtonCaret,
      (hovered || pressed || isOpen) &&
        inlineUnistylesStyle({ backgroundColor: theme.colors.surface2 }),
    ],
    [theme.colors.surface2],
  );

  const menuOnlyTriggerStyle = useCallback(
    ({ hovered, pressed, open: isOpen }: { hovered: boolean; pressed: boolean; open: boolean }) => [
      styles.menuOnlyTrigger,
      (hovered || pressed || isOpen) &&
        inlineUnistylesStyle({ backgroundColor: theme.colors.surface2 }),
    ],
    [theme.colors.surface2],
  );

  if (!primary) {
    return null;
  }

  const panel = (
    <DropdownMenuContent
      align="end"
      minWidth={PANEL_MIN_WIDTH}
      pages={pages}
      testID="changes-primary-cta-menu"
    >
      <GitControlPanel control={control} onRun={runGitAction} />
    </DropdownMenuContent>
  );

  if (menuOnly) {
    return (
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <DropdownMenuTrigger
          testID="changes-actions-menu-trigger"
          style={menuOnlyTriggerStyle}
          accessibilityRole="button"
          accessibilityLabel={t("workspace.header.actions.workspaceActions")}
        >
          <GitBranch size={16} color={theme.colors.foregroundMuted} />
          <ChevronDown size={12} color={theme.colors.foregroundExtraMuted} />
        </DropdownMenuTrigger>
        {panel}
      </DropdownMenu>
    );
  }

  return (
    <View style={styles.splitButton}>
      <Pressable
        testID="changes-primary-cta"
        style={primaryPressableStyle}
        onPress={handlePrimaryPress}
        disabled={primaryDisabled}
        accessibilityRole="button"
        accessibilityLabel={primary.label}
      >
        <PrimaryContent primary={primary} />
      </Pressable>
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <DropdownMenuTrigger
          testID="changes-primary-cta-caret"
          style={caretTriggerStyle}
          accessibilityRole="button"
          accessibilityLabel={t("workspace.git.actions.moreOptions")}
        >
          <ChevronDown size={16} color={theme.colors.foregroundExtraMuted} />
        </DropdownMenuTrigger>
        {panel}
      </DropdownMenu>
    </View>
  );
}

function PrimaryContent({ primary }: { primary: GitControlPrimary }): ReactElement {
  const { theme } = useUnistyles();
  if (primary.kind === "state") {
    const warning = primary.tone === "warning";
    return (
      <View style={styles.splitButtonContent}>
        <ThemedGitBranch
          size={16}
          uniProps={warning ? statusWarningIconColorMapping : mutedIconColorMapping}
        />
        <Text
          style={warning ? styles.splitButtonTextWarning : styles.splitButtonText}
          numberOfLines={1}
        >
          {primary.label}
        </Text>
      </View>
    );
  }
  const { action } = primary;
  if (action.status === "pending") {
    return (
      <LoadingSpinner
        size="small"
        color={theme.colors.foreground}
        style={styles.splitButtonSpinnerOnly}
      />
    );
  }
  return (
    <View style={styles.splitButtonContent}>
      {action.icon}
      <Text style={styles.splitButtonText} numberOfLines={1}>
        {action.status === "success" ? action.successLabel : primary.label}
      </Text>
      {primary.incoming ? <Text style={styles.count}>{`↓${primary.incoming}`}</Text> : null}
      {primary.outgoing ? <Text style={styles.count}>{`↑${primary.outgoing}`}</Text> : null}
    </View>
  );
}

function GitControlPanel({
  control,
  onRun,
}: {
  control: GitControl;
  onRun: (action: GitControlAction) => void;
}): ReactElement {
  return (
    <>
      {control.summary ? <GitControlSummaryRows summary={control.summary} /> : null}
      {control.sections.map((section, index) => (
        <Fragment key={section.id}>
          {control.summary || index > 0 ? <DropdownMenuSeparator /> : null}
          <GitControlSectionRows
            section={section}
            mergeMethod={section.id === "pull-request" ? control.mergeMethod : null}
            onRun={onRun}
          />
        </Fragment>
      ))}
    </>
  );
}

function GitControlSummaryRows({ summary }: { summary: GitControlSummary }): ReactElement {
  const { t } = useTranslation();
  return (
    <>
      <DropdownMenuHint testID="changes-git-summary-branch">
        {summary.base
          ? t("workspace.git.actions.summary.branchInto", {
              branch: summary.branch,
              base: summary.base,
            })
          : summary.branch}
      </DropdownMenuHint>
      {summary.facts.length > 0 ? (
        <DropdownMenuHint testID="changes-git-summary-facts">
          {summary.facts.join(" · ")}
        </DropdownMenuHint>
      ) : null}
    </>
  );
}

function GitControlSectionRows({
  section,
  mergeMethod,
  onRun,
}: {
  section: GitControlSection;
  mergeMethod: GitControlMergeMethod | null;
  onRun: (action: GitControlAction) => void;
}): ReactElement {
  return (
    <>
      {section.label ? <DropdownMenuLabel>{section.label}</DropdownMenuLabel> : null}
      {section.actions.map((action) => (
        <GitControlMenuItem key={action.id} action={action} onSelect={onRun} />
      ))}
      {mergeMethod ? <MergeMethodTrigger mergeMethod={mergeMethod} /> : null}
    </>
  );
}

function GitControlMenuItem({
  action,
  onSelect,
}: {
  action: GitControlAction;
  onSelect: (action: GitControlAction) => void;
}): ReactElement {
  const handleSelect = useCallback(() => onSelect(action), [onSelect, action]);
  const trailing = useMemo(
    () => (action.detail ? <Text style={styles.detail}>{action.detail}</Text> : null),
    [action.detail],
  );
  return (
    <DropdownMenuItem
      testID={`changes-menu-${action.id}`}
      leading={action.icon}
      trailing={trailing}
      description={action.unavailableReason ?? action.description}
      disabled={action.disabled || action.unavailableReason !== undefined}
      destructive={action.destructive}
      status={action.status}
      pendingLabel={action.pendingLabel}
      successLabel={action.successLabel}
      closeOnSelect={closesOnSelect(action)}
      onSelect={handleSelect}
    >
      {action.label}
    </DropdownMenuItem>
  );
}

/** Rows with progress labels stay open to show them; View PR has none and just leaves. */
function closesOnSelect(action: GitControlAction): boolean {
  return (
    action.status === "idle" &&
    action.label === action.pendingLabel &&
    action.label === action.successLabel
  );
}

function MergeMethodTrigger({ mergeMethod }: { mergeMethod: GitControlMergeMethod }) {
  const selected = mergeMethod.options.find((option) => option.method === mergeMethod.selected);
  return (
    <DropdownMenuSubTrigger
      id={MERGE_METHOD_PAGE_ID}
      value={selected?.label}
      leading={GIT_ACTION_ICONS.mergeMethod}
      testID="changes-merge-method-trigger"
    >
      {mergeMethod.label}
    </DropdownMenuSubTrigger>
  );
}

function useMergeMethodPages(
  mergeMethod: GitControlMergeMethod | null,
): readonly MenuPageDefinition[] | undefined {
  return useMemo(
    () =>
      mergeMethod
        ? [
            {
              id: MERGE_METHOD_PAGE_ID,
              title: mergeMethod.label,
              content: <MergeMethodPage mergeMethod={mergeMethod} />,
            },
          ]
        : undefined,
    [mergeMethod],
  );
}

function MergeMethodPage({ mergeMethod }: { mergeMethod: GitControlMergeMethod }): ReactElement {
  return (
    <>
      {mergeMethod.options.map((option) => (
        <MergeMethodOption
          key={option.method}
          method={option.method}
          label={option.label}
          selected={option.method === mergeMethod.selected}
          onSelect={mergeMethod.onSelect}
        />
      ))}
    </>
  );
}

function MergeMethodOption({
  method,
  label,
  selected,
  onSelect,
}: {
  method: CheckoutPrMergeMethod;
  label: string;
  selected: boolean;
  onSelect: (method: CheckoutPrMergeMethod) => void;
}): ReactElement {
  const handleSelect = useCallback(() => onSelect(method), [method, onSelect]);
  return (
    <DropdownMenuItem
      testID={`changes-merge-method-${method}`}
      selected={selected}
      showSelectedCheck
      onSelect={handleSelect}
    >
      {label}
    </DropdownMenuItem>
  );
}

const styles = StyleSheet.create((theme) => ({
  splitButton: {
    height: {
      xs: buttonControlHeight.xs,
      md: HEADER_CONTROL_HEIGHT,
    },
    flexDirection: "row",
    alignItems: "stretch",
    flexShrink: 1,
    minWidth: 0,
    borderRadius: theme.borderRadius.md,
    borderWidth: theme.borderWidth[1],
    borderColor: theme.colors.borderAccent,
    overflow: "hidden",
  },
  splitButtonPrimary: {
    paddingHorizontal: {
      xs: theme.spacing[3],
      md: theme.spacing[2],
    },
    justifyContent: "center",
    flexShrink: 1,
    minWidth: 0,
    position: "relative",
  },
  menuOnlyTrigger: {
    width: {
      xs: 48,
      md: 42,
    },
    height: {
      xs: buttonControlHeight.xs,
      md: HEADER_CONTROL_HEIGHT,
    },
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[1],
    borderRadius: theme.borderRadius.md,
    borderWidth: theme.borderWidth[1],
    borderColor: theme.colors.borderAccent,
  },
  splitButtonPrimaryDisabled: {
    opacity: 0.6,
  },
  splitButtonText: {
    flexShrink: 1,
    fontSize: theme.fontSize.base,
    lineHeight: theme.fontSize.base * 1.5,
    color: theme.colors.foreground,
    fontWeight: theme.fontWeight.normal,
  },
  splitButtonTextWarning: {
    flexShrink: 1,
    fontSize: theme.fontSize.base,
    lineHeight: theme.fontSize.base * 1.5,
    color: theme.colors.statusWarning,
    fontWeight: theme.fontWeight.normal,
  },
  count: {
    flexShrink: 0,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.base * 1.5,
    color: theme.colors.foregroundMuted,
    fontVariant: ["tabular-nums"],
  },
  detail: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
  splitButtonContent: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    minWidth: 0,
    gap: {
      xs: theme.spacing[2],
      md: theme.spacing[1],
    },
  },
  splitButtonSpinnerOnly: {
    transform: [{ scale: 0.8 }],
  },
  splitButtonCaret: {
    width: {
      xs: buttonControlHeight.xs,
      md: HEADER_CONTROL_HEIGHT,
    },
    alignItems: "center",
    justifyContent: "center",
    borderLeftWidth: theme.borderWidth[1],
    borderLeftColor: theme.colors.borderAccent,
  },
}));
