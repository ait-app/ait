import { getFeatureIconColor } from "./utils";
import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { ChevronRight, Settings2, Terminal } from "lucide-react-native";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { isPresetOwnedFeature } from "@/agent-controls/model-presets";
import { getAgentFeatureIcon } from "@/agent-controls/icons";
import { presentFeature, presentFeatureCommand } from "@/agent-controls/feature-menu-labels";
import { AgentControlTrigger } from "./control";
import { Combobox, ComboboxItem, type ComboboxOption } from "@/components/ui/combobox";
import type { AutocompleteOption } from "@/components/ui/autocomplete";
import { useAgentAutocomplete, type AgentAutocompleteOption } from "@/hooks/use-agent-autocomplete";
import type { DraftCommandConfig } from "@/hooks/use-agent-commands-query";

interface FeatureCommandsContextValue {
  serverId: string;
  agentId: string;
  draftConfig?: DraftCommandConfig;
  setUserInput: (text: string) => void;
}

const FeatureCommandsContext = createContext<FeatureCommandsContextValue | null>(null);
export const ComposerFeatureCommandsProvider = FeatureCommandsContext.Provider;

interface FeaturesControlProps {
  provider?: string;
  features?: AgentFeature[];
  onSetFeature?: (id: string, value: unknown) => void;
  disabled?: boolean;
  onClose?: () => void;
  surface?: "toolbar" | "sheet";
}

export function AgentFeaturesControl(props: FeaturesControlProps) {
  const context = useContext(FeatureCommandsContext);
  const [open, setOpen] = useState(false);
  const changeOpen = useCallback(
    (next: boolean) => {
      setOpen(next);
      if (!next) props.onClose?.();
    },
    [props.onClose],
  );
  const menuProps = { ...props, open, changeOpen };
  return context ? (
    <ConnectedFeaturesMenu {...menuProps} context={context} />
  ) : (
    <FeaturesMenu {...menuProps} />
  );
}

interface FeaturesMenuProps extends FeaturesControlProps {
  open: boolean;
  changeOpen: (open: boolean) => void;
  commands?: AgentAutocompleteOption[];
  onSelectCommand?: (option: AutocompleteOption) => void;
  commandStatus?: string;
}

function ConnectedFeaturesMenu({
  context,
  ...props
}: FeaturesMenuProps & { context: FeatureCommandsContextValue }) {
  // Reuse the slash menu's catalog, ownership checks and BTW routing. Commands are inserted
  // for the user to submit; opening this menu never executes a command or archives a session.
  const commands = useAgentAutocomplete({
    ...context,
    userInput: props.open ? "/" : "",
    cursorIndex: props.open ? 1 : 0,
    canExecuteClientSlashCommand: false,
  });
  return (
    <FeaturesMenu
      {...props}
      commands={commands.options}
      onSelectCommand={commands.onSelectOption}
      commandStatus={
        commands.errorMessage ?? (commands.isLoading ? commands.loadingText : undefined)
      }
    />
  );
}

function FeaturesMenu({
  provider,
  features,
  onSetFeature,
  disabled = false,
  surface = "toolbar",
  open,
  changeOpen,
  commands = [],
  onSelectCommand,
  commandStatus,
}: FeaturesMenuProps) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const anchorRef = useRef<View>(null);
  const [selectedFeatureId, setSelectedFeatureId] = useState<string | null>(null);
  const availableFeatures = useMemo(
    () => (features ?? []).filter((feature) => !isPresetOwnedFeature(feature.id)),
    [features],
  );
  const selectedFeature = availableFeatures.find((feature) => feature.id === selectedFeatureId);
  const options = useMemo<ComboboxOption[]>(() => {
    if (selectedFeature?.type === "select") return selectedFeature.options;
    return [
      ...availableFeatures.map((feature) => ({
        id: `feature:${feature.id}`,
        ...presentFeature(feature, provider, t),
      })),
      ...commands.flatMap((command) => {
        const presentation = presentFeatureCommand(command, provider, t);
        return presentation ? [{ id: `command:${command.id}`, ...presentation }] : [];
      }),
    ];
  }, [availableFeatures, commands, provider, selectedFeature, t]);

  const handleOpenChange = useCallback(
    (next: boolean) => {
      setSelectedFeatureId(null);
      changeOpen(next);
    },
    [changeOpen],
  );
  const handleSelect = useCallback(
    (id: string) => {
      if (disabled) return;
      if (selectedFeature?.type === "select") {
        onSetFeature?.(selectedFeature.id, id);
        setSelectedFeatureId(null);
        return;
      }
      const feature = availableFeatures.find((item) => id === `feature:${item.id}`);
      if (feature) {
        if (!onSetFeature) return;
        if (feature.type === "toggle") onSetFeature(feature.id, !feature.value);
        else setSelectedFeatureId(feature.id);
        return;
      }
      const command = commands.find((item) => id === `command:${item.id}`);
      if (command) {
        onSelectCommand?.(command);
        handleOpenChange(false);
      }
    },
    [
      availableFeatures,
      commands,
      disabled,
      handleOpenChange,
      onSelectCommand,
      onSetFeature,
      selectedFeature,
    ],
  );
  const renderOption = useCallback(
    ({
      option,
      selected,
      active,
      onPress,
    }: {
      option: ComboboxOption;
      selected: boolean;
      active: boolean;
      onPress: () => void;
    }) => {
      const feature = availableFeatures.find((item) => option.id === `feature:${item.id}`);
      const Icon = feature ? getAgentFeatureIcon(feature.icon, feature.id) : Terminal;
      const value =
        feature?.type === "select"
          ? feature.options.find((item) => item.id === feature.value)?.label
          : undefined;
      return (
        <ComboboxItem
          label={option.label}
          description={option.description}
          selected={feature?.type === "toggle" ? feature.value : selected}
          active={active}
          onPress={onPress}
          disabled={disabled || Boolean(feature && !onSetFeature)}
          leadingSlot={
            <Icon
              size={16}
              color={feature ? getFeatureIconColor(feature, theme.colors) : theme.colors.foreground}
            />
          }
          trailingSlot={
            feature?.type === "select" ? (
              <View style={styles.value}>
                <Text numberOfLines={1} style={styles.valueText}>
                  {value}
                </Text>
                <ChevronRight size={16} color={theme.colors.foregroundMuted} />
              </View>
            ) : undefined
          }
          testID={feature ? `agent-feature-${feature.id}` : option.id}
        />
      );
    },
    [availableFeatures, disabled, onSetFeature, theme.colors, theme.colors.foregroundMuted],
  );

  if (!availableFeatures.length && !onSelectCommand) return null;
  return (
    <>
      <AgentControlTrigger
        ref={anchorRef}
        icon={Settings2}
        surface={surface}
        label={t("agentControls.features.title")}
        value={
          surface === "sheet"
            ? availableFeatures
                .filter((feature) => feature.type === "toggle" && feature.value)
                .map((feature) => presentFeature(feature, provider, t).label)
                .join(", ")
            : undefined
        }
        showToolbarLabel={false}
        open={open}
        disabled={disabled}
        onPress={() => handleOpenChange(!open)}
        accessibilityLabel={t("agentControls.features.open")}
        testID="agent-controls-features"
      />
      <Combobox
        key={selectedFeatureId ?? "features"}
        options={options}
        value={selectedFeature?.type === "select" ? (selectedFeature.value ?? "") : ""}
        onSelect={handleSelect}
        open={open}
        onOpenChange={handleOpenChange}
        anchorRef={anchorRef}
        desktopPlacement="top-start"
        desktopMinWidth={260}
        searchPlaceholder={t("agentControls.features.searchPlaceholder")}
        keepOpenOnSelect
        renderOption={renderOption}
        header={
          selectedFeature
            ? {
                title: presentFeature(selectedFeature, provider, t).label,
                back: { onPress: () => setSelectedFeatureId(null) },
              }
            : undefined
        }
        footer={commandStatus ? <Text style={styles.status}>{commandStatus}</Text> : undefined}
      />
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  value: { flexDirection: "row", alignItems: "center", gap: theme.spacing[2], maxWidth: 160 },
  valueText: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base, flexShrink: 1 },
  status: {
    color: theme.colors.foregroundMuted,
    padding: theme.spacing[3],
    fontSize: theme.fontSize.sm,
  },
}));
