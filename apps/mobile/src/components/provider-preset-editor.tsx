import { useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Trash2 } from "lucide-react-native";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { ProviderPreset } from "@ait/protocol/messages";
import { AdaptiveModalSheet, AdaptiveTextInput } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { SelectField } from "@/components/ui/select-field";
import { formatThinkingOptionLabel } from "@/agent-controls/labels";
import { getAgentFeatureIcon } from "@/agent-controls/icons";
import { useIsCompactFormFactor } from "@/constants/layout";
import { toErrorMessage } from "@/utils/error-messages";
import {
  decodePresetRoles,
  encodePresetRoles,
  nativeRolePreview,
  presetModelLabel,
  presetRoleFeatures,
  roleModelChoices,
  type PresetRoleRow,
  type RoleFeature,
} from "./provider-preset-form";

const roleName = (feature: RoleFeature | undefined, key: string) =>
  feature?.label?.replace(/ model(?: \([^)]*\))?$/i, "") ?? key.replace(/^modelRole\./, "");

export function ProviderPresetEditor({
  preset,
  models,
  features,
  canRename,
  canSave,
  native = false,
  readOnly = false,
  readOnlyReason,
  existingNames,
  onSave,
  onClose,
}: {
  preset: ProviderPreset;
  models: readonly AgentModelDefinition[];
  features: readonly AgentFeature[];
  canRename: boolean;
  canSave: boolean;
  native?: boolean;
  readOnly?: boolean;
  readOnlyReason?: string;
  existingNames: readonly string[];
  onSave: (preset: ProviderPreset) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const compact = useIsCompactFormFactor();
  const [name, setName] = useState(preset.name);
  const [model, setModel] = useState(preset.model);
  const [thinking, setThinking] = useState(preset.thinkingOptionId ?? "");
  const [roles, setRoles] = useState(() => decodePresetRoles(preset));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [details, setDetails] = useState(false);
  const nextRole = useRef(0);
  const seededDefaults = useRef(false);
  const isNew = !preset.name;
  const roleFeatures = useMemo(() => presetRoleFeatures(features), [features]);
  useEffect(() => {
    if (!isNew || seededDefaults.current || !roleFeatures.length) return;
    seededDefaults.current = true;
    setRoles(
      roleFeatures
        .filter((feature) => {
          const option = feature.options.find((item) => item.id === "native");
          return typeof option?.metadata?.nativeSelector === "string";
        })
        .map((feature) => ({
          id: `initial-${feature.id}`,
          role: feature.id,
          model: "native",
          thinking: "",
        })),
    );
  }, [isNew, roleFeatures]);
  const takenName = isNew && existingNames.includes(name.trim());
  const selectedModel = models.find((entry) => entry.id === model);
  const modelOptions = models.map((entry) => ({
    id: entry.id,
    value: entry.id,
    label: entry.label,
  }));
  if (model && !modelOptions.some((item) => item.id === model))
    modelOptions.push({ id: model, value: model, label: presetModelLabel(model, models) });
  const efforts = [...(selectedModel?.thinkingOptions ?? [])];
  if (thinking && !efforts.some((item) => item.id === thinking))
    efforts.push({ id: thinking, label: thinking });
  const availableRoles = roleFeatures.filter(
    (feature) => !roles.some((row) => row.role === feature.id),
  );
  const orderedRoles = [...roles].sort(
    (a, b) =>
      roleFeatures.findIndex((item) => item.id === a.role) -
      roleFeatures.findIndex((item) => item.id === b.role),
  );
  const canSubmit = canSave && !readOnly && !!name.trim() && !!model && !takenName;
  const close = () => {
    if (!saving) onClose();
  };
  const save = async () => {
    if (!canSubmit || saving) return;
    setSaving(true);
    setError(null);
    try {
      await onSave({
        name: name.trim(),
        model,
        ...(thinking ? { thinkingOptionId: thinking } : {}),
        featureValues: encodePresetRoles(
          preset,
          roles,
          native
            ? [
                ...new Set([
                  ...roleFeatures.map((feature) => feature.id),
                  ...Object.keys(preset.featureValues ?? {}).filter((key) =>
                    key.startsWith("modelRole."),
                  ),
                ]),
              ]
            : [],
        ),
      });
      onClose();
    } catch (cause) {
      setError(toErrorMessage(cause));
    } finally {
      setSaving(false);
    }
  };
  const mainThinking = (
    <SelectField
      field={false}
      size="sm"
      label={t("settings.providers.presets.thinking")}
      value={thinking}
      selectedDisplay={{
        label: thinking
          ? formatThinkingOptionLabel(
              efforts.find((item) => item.id === thinking) ?? { id: thinking },
              t,
            )
          : t("providerControl.roleDefaultShort"),
      }}
      options={[
        { id: "", value: "", label: t("providerControl.defaultThinking") },
        ...efforts.map((item) => ({
          id: item.id,
          value: item.id,
          label: formatThinkingOptionLabel(item, t),
        })),
      ]}
      onChange={setThinking}
      disabled={readOnly || saving}
      placeholder={t("settings.providers.presets.thinking")}
      emptyText={t("providerControl.defaultThinking")}
      testID="provider-preset-thinking-options"
    />
  );
  const footer = (
    <View style={styles.actions}>
      <Button variant="secondary" size="sm" onPress={close} disabled={saving}>
        {t(readOnly ? "common.actions.close" : "common.actions.cancel")}
      </Button>
      {!readOnly ? (
        <Button
          size="sm"
          onPress={() => void save()}
          loading={saving}
          disabled={!canSubmit || saving}
          testID="provider-preset-save"
        >
          {t(saving ? "settings.providers.presets.saving" : "common.actions.save")}
        </Button>
      ) : null}
    </View>
  );
  return (
    <AdaptiveModalSheet
      visible
      footer={footer}
      footerContainerStyle={styles.footer}
      onClose={close}
      desktopMaxWidth={620}
      snapPoints={["80%", "95%"]}
      testID="provider-preset-editor"
      header={{
        title: readOnly
          ? t("providerControl.viewPreset", { name: preset.name })
          : isNew
            ? t("settings.providers.presets.createTitle")
            : t("settings.providers.presets.editTitle", { name: preset.name }),
      }}
    >
      <View style={styles.form}>
        {readOnly && readOnlyReason ? <Text style={styles.muted}>{readOnlyReason}</Text> : null}
        <Text style={styles.label}>{t("settings.providers.presets.name")}</Text>
        <AdaptiveTextInput
          initialValue={name}
          resetKey={preset.name}
          onChangeText={setName}
          editable={!readOnly && (canRename || isNew)}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder={t("settings.providers.presets.namePlaceholder")}
          placeholderTextColor={theme.colors.foregroundMuted}
          style={styles.input}
        />
        {takenName ? (
          <Text style={styles.error} testID="provider-preset-name-taken">
            {t("settings.providers.presets.nameTaken", { name })}
          </Text>
        ) : null}
        <View style={styles.tableHeader}>
          <Text style={styles.roleName}>{t("providerControl.role")}</Text>
          <Text style={styles.modelHeading}>{t("settings.providers.presets.model")}</Text>
          {!compact ? (
            <Text style={styles.effortHeading}>{t("providerControl.roleThinking")}</Text>
          ) : null}
          <View style={styles.removeSpace} />
        </View>
        <View style={styles.table}>
          <View style={styles.roleRow}>
            <Text style={styles.roleName}>DEFAULT</Text>
            <View style={styles.modelCell}>
              <SelectField
                field={false}
                size="sm"
                label={t("settings.providers.presets.model")}
                value={model || null}
                selectedDisplay={model ? { label: presetModelLabel(model, models) } : null}
                options={modelOptions}
                disabled={readOnly || saving}
                onChange={(value) => {
                  setModel(value);
                  if (
                    !models
                      .find((entry) => entry.id === value)
                      ?.thinkingOptions?.some((entry) => entry.id === thinking)
                  )
                    setThinking("");
                }}
                placeholder={t("settings.providers.presets.model")}
                emptyText={t("providerControl.noModels")}
                searchable
                testID="provider-preset-model-options"
              />
              {compact ? (
                <View style={styles.compactThinking}>
                  <Text style={styles.muted}>{t("providerControl.roleThinking")}</Text>
                  {mainThinking}
                </View>
              ) : null}
            </View>
            {!compact ? <View style={styles.effortCell}>{mainThinking}</View> : null}
            <View style={styles.removeSpace} />
          </View>
          {orderedRoles.map((row) => (
            <RoleAssignment
              key={row.id}
              row={row}
              feature={roleFeatures.find((item) => item.id === row.role)}
              models={models}
              disabled={readOnly || saving}
              onChange={(next) =>
                setRoles((current) => current.map((item) => (item.id === row.id ? next : item)))
              }
              onRemove={() => setRoles((current) => current.filter((item) => item.id !== row.id))}
            />
          ))}
        </View>
        {!readOnly && availableRoles.length ? (
          <SelectField
            field={false}
            size="sm"
            label={t("providerControl.addRole")}
            value={null}
            selectedDisplay={null}
            options={availableRoles.map((feature) => ({
              id: feature.id,
              value: feature.id,
              label: `${feature.id.slice(10).toUpperCase()} · ${roleName(feature, feature.id)}`,
            }))}
            onChange={(role) =>
              setRoles((current) => [
                ...current,
                { id: `added-${nextRole.current++}`, role, model: "native", thinking: "" },
              ])
            }
            disabled={saving}
            placeholder={t("providerControl.newRole")}
            emptyText={t("providerControl.modelRoles")}
            searchable
            testID="provider-preset-add-role"
          />
        ) : null}
        <Button variant="ghost" size="xs" onPress={() => setDetails(!details)}>
          {t("providerControl.details")}
        </Button>
        {details ? (
          <View style={styles.form}>
            <Text style={styles.muted}>{t("providerControl.roleFormHint")}</Text>
            {native ? (
              <Text style={styles.muted}>{t("providerControl.nativePresetDefaults")}</Text>
            ) : null}
            {!canRename && !isNew ? (
              <Text style={styles.muted}>{t("settings.providers.presets.noRename")}</Text>
            ) : null}
          </View>
        ) : null}
        {!readOnly && !canSave ? (
          <Text style={styles.error}>{t("providerControl.presetReadOnly")}</Text>
        ) : null}
        {error ? (
          <Text accessibilityRole="alert" style={styles.error}>
            {error}
          </Text>
        ) : null}
      </View>
    </AdaptiveModalSheet>
  );
}

function RoleAssignment({
  row,
  feature,
  models,
  disabled,
  onChange,
  onRemove,
}: {
  row: PresetRoleRow;
  feature: RoleFeature | undefined;
  models: readonly AgentModelDefinition[];
  disabled: boolean;
  onChange: (row: PresetRoleRow) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const compact = useIsCompactFormFactor();
  const choices = roleModelChoices(feature, models);
  const inherited = nativeRolePreview(feature, models);
  const options = [
    { id: "native", value: "native", label: t("providerControl.roleNative") },
    { id: "unset", value: "unset", label: t("providerControl.roleUnset") },
    ...choices.map((item) => ({ id: item.id, value: item.id, label: item.label })),
  ];
  if (!options.some((item) => item.id === row.model))
    options.push({ id: row.model, value: row.model, label: presetModelLabel(row.model, models) });
  const automatic = row.model === "native";
  const unassigned = row.model === "unset";
  const label = automatic
    ? inherited
      ? t("providerControl.roleInheritedModel", { model: inherited.label })
      : t("providerControl.roleNative")
    : options.find((item) => item.id === row.model)!.label;
  const efforts = [...(choices.find((item) => item.id === row.model)?.thinkingOptions ?? [])];
  if (row.thinking && !efforts.some((item) => item.id === row.thinking))
    efforts.push({ id: row.thinking, label: row.thinking });
  const effort = automatic ? (inherited?.thinking ?? "") : row.thinking;
  const roleThinking = (
    <SelectField
      field={false}
      size="sm"
      label={t("settings.providers.presets.thinking")}
      value={effort}
      selectedDisplay={{
        label: effort
          ? formatThinkingOptionLabel({ id: effort }, t)
          : t("providerControl.roleDefaultShort"),
      }}
      options={[
        { id: "", value: "", label: t("providerControl.roleDefaultThinking") },
        ...efforts.map((item) => ({
          id: item.id,
          value: item.id,
          label: formatThinkingOptionLabel(item, t),
        })),
      ]}
      onChange={(thinking) => onChange({ ...row, thinking })}
      disabled={disabled || automatic || unassigned}
      placeholder={t("settings.providers.presets.thinking")}
      emptyText={t("providerControl.roleDefaultThinking")}
      testID={`preset-role-thinking-${row.id}`}
    />
  );
  const Icon = getAgentFeatureIcon(undefined, row.role);
  return (
    <View style={styles.roleRow} testID={`provider-preset-role-${row.id}`}>
      <View style={styles.roleIdentity}>
        <Icon size={12} color={styles.muted.color} />
        <Text
          style={styles.roleCode}
          testID={`preset-role-name-${row.id}`}
          accessibilityLabel={roleName(feature, row.role)}
        >
          {row.role.slice(10).toUpperCase()}
        </Text>
      </View>
      <View style={styles.modelCell}>
        <SelectField
          field={false}
          size="sm"
          label={roleName(feature, row.role)}
          value={row.model}
          selectedDisplay={{ label }}
          options={options}
          onChange={(model) =>
            onChange({ ...row, model, thinking: model === row.model ? row.thinking : "" })
          }
          disabled={disabled}
          searchable
          placeholder={t("settings.providers.presets.model")}
          emptyText={t("providerControl.noModels")}
          testID={`preset-role-model-${row.id}`}
        />
        {compact ? (
          <View style={styles.compactThinking}>
            <Text style={styles.muted}>{t("providerControl.roleThinking")}</Text>
            {roleThinking}
          </View>
        ) : null}
      </View>
      {!compact ? <View style={styles.effortCell}>{roleThinking}</View> : null}
      <View style={styles.removeSpace}>
        {!disabled ? (
          <Button
            variant="ghost"
            size="sm"
            leftIcon={Trash2}
            accessibilityLabel={t("providerControl.removeRole")}
            onPress={onRemove}
            testID={`preset-role-remove-${row.id}`}
          />
        ) : null}
      </View>
    </View>
  );
}
const styles = StyleSheet.create((theme) => ({
  form: { gap: 12 },
  label: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  muted: { color: theme.colors.foregroundMuted, fontSize: 12 },
  input: {
    padding: 12,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 10,
    color: theme.colors.foreground,
    backgroundColor: theme.colors.surface2,
  },
  table: { borderWidth: 1, borderColor: theme.colors.border, borderRadius: 10, overflow: "hidden" },
  tableHeader: { flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8 },
  roleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 8,
    paddingVertical: 6,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  roleName: { width: 86, color: theme.colors.foreground, fontSize: 12, fontWeight: "600" },
  roleIdentity: { width: 86, flexDirection: "row", alignItems: "center", gap: 4 },
  roleCode: { color: theme.colors.foreground, fontSize: 11, fontWeight: "600", flexShrink: 1 },
  modelHeading: { flex: 1, color: theme.colors.foregroundMuted, fontSize: 12 },
  effortHeading: { width: 110, color: theme.colors.foregroundMuted, fontSize: 12 },
  modelCell: { flex: 1, minWidth: 0, gap: 4 },
  compactThinking: { flexDirection: "row", alignItems: "center", gap: 8 },
  effortCell: { width: 110 },
  removeSpace: { width: 26 },
  actions: { flexDirection: "row", justifyContent: "flex-end", gap: 8, flex: 1 },
  footer: { alignSelf: "stretch" },
  error: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
}));
