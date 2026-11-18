import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { ArrowDown, ArrowUp, Trash2 } from "lucide-react-native";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { ProviderPreset } from "@ait/protocol/messages";
import { AdaptiveModalSheet, AdaptiveTextInput } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { SelectField } from "@/components/ui/select-field";
import { formatThinkingOptionLabel } from "@/agent-controls/labels";
import { useIsCompactFormFactor } from "@/constants/layout";
import { toErrorMessage } from "@/utils/error-messages";
import {
  buildModelCatalogIndex,
  decodePresetFallbacks,
  decodePresetRoles,
  defaultRoleFeature,
  encodePresetRoles,
  inheritedRoleFallbacks,
  nativeRolePreview,
  presetModelDisplay,
  presetRoleFeatures,
  presetSupportsFallbacks,
  roleFallbackChoices,
  roleModelChoices,
  type ModelCatalogIndex,
  type PresetFallbackItem,
  type PresetRoleRow,
  type RoleFeature,
  type RoleModelChoice,
} from "./provider-preset-form";

const ROLE_PREFIX = "modelRole.";

function roleLabel(feature: RoleFeature | undefined, key: string): string {
  return feature?.label?.replace(/ model(?: \([^)]*\))?$/i, "") ?? key.replace(/^modelRole\./, "");
}

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
  const [fallbacks, setFallbacks] = useState(() => decodePresetFallbacks(preset));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nextId = useRef(0);
  const seededDefaults = useRef(false);
  const isNew = !preset.name;

  const catalogIndex = useMemo(() => buildModelCatalogIndex(models), [models]);
  const defaultFeature = useMemo(() => defaultRoleFeature(features), [features]);
  const roleFeatures = useMemo(() => presetRoleFeatures(features), [features]);
  const supportsFallbacks = useMemo(
    () => presetSupportsFallbacks(features, preset),
    [features, preset],
  );
  const roleFeatureMap = useMemo(() => {
    const map = new Map<string, RoleFeature>();
    for (const item of roleFeatures) {
      map.set(item.id, item);
    }
    return map;
  }, [roleFeatures]);

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

  const trimmedName = name.trim();
  const takenName = trimmedName !== preset.name && existingNames.includes(trimmedName);
  const invalidNativeName =
    native && Boolean(trimmedName) && !/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(trimmedName);

  const selectedModel = useMemo(() => catalogIndex.byId.get(model)?.entry, [catalogIndex, model]);
  const modelOptions = useMemo(() => {
    if (!model || catalogIndex.byId.has(model)) {
      return catalogIndex.modelOptions;
    }
    const unresolved = presetModelDisplay(model, catalogIndex);
    return [
      ...catalogIndex.modelOptions,
      {
        id: model,
        value: model,
        label: unresolved.label,
        ...(unresolved.description ? { description: unresolved.description } : {}),
      },
    ];
  }, [catalogIndex, model]);

  const efforts = useMemo(() => {
    const base = [...(selectedModel?.thinkingOptions ?? [])];
    if (thinking && !base.some((item) => item.id === thinking)) {
      base.push({ id: thinking, label: thinking });
    }
    return base;
  }, [selectedModel, thinking]);

  const defaultChoices = useMemo(
    () => roleModelChoices(defaultFeature, catalogIndex),
    [catalogIndex, defaultFeature],
  );
  const defaultFallbackOptions = useMemo(
    () => roleFallbackChoices(defaultChoices, catalogIndex),
    [catalogIndex, defaultChoices],
  );
  const defaultInheritedFallbacks = useMemo(
    () => inheritedRoleFallbacks(defaultFeature, catalogIndex),
    [catalogIndex, defaultFeature],
  );

  const availableRoles = useMemo(
    () => roleFeatures.filter((feature) => !roles.some((row) => row.role === feature.id)),
    [roleFeatures, roles],
  );
  const orderedRoles = useMemo(() => {
    const order = new Map(roleFeatures.map((item, idx) => [item.id, idx]));
    return [...roles].sort(
      (left, right) => (order.get(left.role) ?? 9999) - (order.get(right.role) ?? 9999),
    );
  }, [roleFeatures, roles]);

  const setRoleFallbacks = useCallback((roleId: string, nextItems: PresetFallbackItem[]) => {
    setFallbacks((current) => {
      const next = { ...current };
      if (nextItems.length === 0) {
        delete next[roleId];
      } else {
        next[roleId] = nextItems;
      }
      return next;
    });
  }, []);

  const handleUpdateRole = useCallback((nextRow: PresetRoleRow) => {
    setRoles((current) => current.map((item) => (item.id === nextRow.id ? nextRow : item)));
  }, []);

  const handleRemoveRole = useCallback((row: PresetRoleRow) => {
    const roleId = row.role.slice(ROLE_PREFIX.length);
    setRoles((current) => current.filter((item) => item.id !== row.id));
    setFallbacks((current) => {
      if (!(roleId in current)) return current;
      const next = { ...current };
      delete next[roleId];
      return next;
    });
  }, []);

  const createFallbackId = useCallback(() => `added-fallback-${nextId.current++}`, []);

  const canSubmit =
    canSave &&
    !readOnly &&
    Boolean(trimmedName) &&
    Boolean(model) &&
    !takenName &&
    !invalidNativeName;

  const close = useCallback(() => {
    if (!saving) onClose();
  }, [onClose, saving]);

  const save = async () => {
    if (!canSubmit || saving) return;
    setSaving(true);
    setError(null);
    try {
      await onSave({
        name: trimmedName,
        model,
        ...(thinking ? { thinkingOptionId: thinking } : {}),
        featureValues: encodePresetRoles(preset, roles, fallbacks),
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

  const defaultFallbacks = fallbacks.default ?? [];

  return (
    <AdaptiveModalSheet
      visible
      footer={footer}
      footerContainerStyle={styles.footer}
      onClose={close}
      desktopMaxWidth={660}
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
        {readOnly && readOnlyReason ? (
          <Text style={styles.bannerText}>{readOnlyReason}</Text>
        ) : null}
        <View style={styles.fieldGroup}>
          <Text style={styles.label}>{t("settings.providers.presets.name")}</Text>
          <AdaptiveTextInput
            initialValue={name}
            resetKey={preset.name}
            onChangeText={setName}
            editable={!readOnly && (canRename || isNew)}
            maxLength={native ? 128 : undefined}
            autoCapitalize="none"
            autoCorrect={false}
            accessibilityLabel={t("settings.providers.presets.name")}
            testID="provider-preset-name"
            placeholder={native ? "TeamDefault" : t("settings.providers.presets.namePlaceholder")}
            placeholderTextColor={theme.colors.foregroundMuted}
            style={[styles.input, (!canRename && !isNew) || readOnly ? styles.inputReadOnly : null]}
          />
          {invalidNativeName ? (
            <Text
              style={styles.error}
              accessibilityRole="alert"
              testID="provider-preset-invalid-name"
            >
              {t("settings.providers.presets.nativeNameInvalid")}
            </Text>
          ) : null}
          {takenName ? (
            <Text style={styles.error} testID="provider-preset-name-taken">
              {t("settings.providers.presets.nameTaken", { name: trimmedName })}
            </Text>
          ) : null}
        </View>

        <View style={styles.sectionGroup}>
          <View style={styles.tableHeader}>
            <Text style={styles.roleHeading}>{t("providerControl.role")}</Text>
            <Text style={styles.modelHeading}>{t("settings.providers.presets.model")}</Text>
            {!compact ? (
              <Text style={styles.effortHeading}>{t("providerControl.roleThinking")}</Text>
            ) : null}
            <View style={styles.removeSpace} />
          </View>
          <View style={styles.table}>
            <View style={styles.roleGroup}>
              <View style={styles.roleRow}>
                <View style={styles.roleIdentity}>
                  <Text style={styles.roleTitle} numberOfLines={1}>
                    {t("providerControl.roleNames.default")}
                  </Text>
                </View>
                <View style={styles.modelCell}>
                  <SelectField
                    field={false}
                    size="sm"
                    label={t("settings.providers.presets.model")}
                    value={model || null}
                    selectedDisplay={model ? presetModelDisplay(model, catalogIndex) : null}
                    options={modelOptions}
                    disabled={readOnly || saving}
                    onChange={(value) => {
                      setModel(value);
                      if (
                        !catalogIndex.byId
                          .get(value)
                          ?.entry.thinkingOptions?.some((entry) => entry.id === thinking)
                      ) {
                        setThinking("");
                      }
                    }}
                    placeholder={t("settings.providers.presets.model")}
                    emptyText={t("providerControl.noModels")}
                    searchable
                    testID="provider-preset-model-options"
                  />
                  {compact ? <View style={styles.compactThinking}>{mainThinking}</View> : null}
                </View>
                {!compact ? <View style={styles.effortCell}>{mainThinking}</View> : null}
                <View style={styles.removeSpace} />
              </View>
              {supportsFallbacks ? (
                <FallbackChainEditor
                  roleId="default"
                  items={defaultFallbacks}
                  choices={defaultFallbackOptions}
                  inherited={defaultInheritedFallbacks}
                  catalogIndex={catalogIndex}
                  disabled={readOnly || saving}
                  compact={compact}
                  createId={createFallbackId}
                  onChange={setRoleFallbacks}
                />
              ) : null}
            </View>

            {orderedRoles.map((row) => {
              const roleId = row.role.slice(ROLE_PREFIX.length);
              return (
                <RoleAssignment
                  key={row.id}
                  row={row}
                  roleId={roleId}
                  feature={roleFeatureMap.get(row.role)}
                  catalogIndex={catalogIndex}
                  supportsFallbacks={supportsFallbacks}
                  fallbacks={fallbacks[roleId] ?? EMPTY_FALLBACKS}
                  disabled={readOnly || saving}
                  compact={compact}
                  createFallbackId={createFallbackId}
                  onChange={handleUpdateRole}
                  onChangeFallbacks={setRoleFallbacks}
                  onRemove={handleRemoveRole}
                />
              );
            })}
          </View>
        </View>

        {!readOnly && availableRoles.length > 0 ? (
          <SelectField
            field={false}
            size="sm"
            label={t("providerControl.addRole")}
            value={null}
            selectedDisplay={null}
            options={availableRoles.map((feature) => {
              const code = feature.id.slice(ROLE_PREFIX.length);
              const friendly = roleLabel(feature, feature.id);
              return {
                id: feature.id,
                value: feature.id,
                label:
                  friendly.toLowerCase() === code.toLowerCase()
                    ? friendly
                    : `${friendly} (${code})`,
              };
            })}
            onChange={(role) =>
              setRoles((current) => [
                ...current,
                { id: `added-${nextId.current++}`, role, model: "native", thinking: "" },
              ])
            }
            disabled={saving}
            placeholder={t("providerControl.newRole")}
            emptyText={t("providerControl.modelRoles")}
            searchable
            testID="provider-preset-add-role"
          />
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

const EMPTY_FALLBACKS: readonly PresetFallbackItem[] = [];

const RoleAssignment = memo(function RoleAssignment({
  row,
  roleId,
  feature,
  catalogIndex,
  supportsFallbacks,
  fallbacks,
  disabled,
  compact,
  createFallbackId,
  onChange,
  onChangeFallbacks,
  onRemove,
}: {
  row: PresetRoleRow;
  roleId: string;
  feature: RoleFeature | undefined;
  catalogIndex: ModelCatalogIndex;
  supportsFallbacks: boolean;
  fallbacks: readonly PresetFallbackItem[];
  disabled: boolean;
  compact: boolean;
  createFallbackId: () => string;
  onChange: (row: PresetRoleRow) => void;
  onChangeFallbacks: (roleId: string, items: PresetFallbackItem[]) => void;
  onRemove: (row: PresetRoleRow) => void;
}) {
  const { t } = useTranslation();
  const choices = useMemo(() => roleModelChoices(feature, catalogIndex), [catalogIndex, feature]);
  const fallbackChoices = useMemo(
    () => roleFallbackChoices(choices, catalogIndex),
    [catalogIndex, choices],
  );
  const inherited = useMemo(
    () => nativeRolePreview(feature, catalogIndex),
    [catalogIndex, feature],
  );
  const inheritedFallbacks = useMemo(
    () => inheritedRoleFallbacks(feature, catalogIndex),
    [catalogIndex, feature],
  );

  const options = useMemo(() => {
    const list: { id: string; value: string; label: string; description?: string }[] = [
      { id: "native", value: "native", label: t("providerControl.roleNative") },
      { id: "unset", value: "unset", label: t("providerControl.roleUnset") },
      ...choices.map((item) => ({
        id: item.id,
        value: item.id,
        label: item.label,
        ...(item.description ? { description: item.description } : {}),
      })),
    ];
    if (!list.some((item) => item.id === row.model)) {
      const unresolved = presetModelDisplay(row.model, catalogIndex);
      list.push({
        id: row.model,
        value: row.model,
        label: unresolved.label,
        ...(unresolved.description ? { description: unresolved.description } : {}),
      });
    }
    return list;
  }, [catalogIndex, choices, row.model, t]);

  const automatic = row.model === "native";
  const unassigned = row.model === "unset";
  const selectedOption = options.find((item) => item.id === row.model);
  const displayLabel = automatic
    ? inherited
      ? t("providerControl.roleInheritedModel", { model: inherited.label })
      : t("providerControl.roleNative")
    : (selectedOption?.label ?? row.model);

  const efforts = useMemo(() => {
    const base = [...(choices.find((item) => item.id === row.model)?.thinkingOptions ?? [])];
    if (row.thinking && !base.some((item) => item.id === row.thinking)) {
      base.push({ id: row.thinking, label: row.thinking });
    }
    return base;
  }, [choices, row.model, row.thinking]);

  const effort = automatic ? (inherited?.thinking ?? "") : row.thinking;
  const friendlyName = roleLabel(feature, row.role);

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
      onChange={(nextThinking) => onChange({ ...row, thinking: nextThinking })}
      disabled={disabled || automatic || unassigned}
      placeholder={t("settings.providers.presets.thinking")}
      emptyText={t("providerControl.roleDefaultThinking")}
      testID={`preset-role-thinking-${row.id}`}
    />
  );

  return (
    <View style={styles.roleGroup} testID={`provider-preset-role-${row.id}`}>
      <View style={styles.roleRow}>
        <View style={styles.roleIdentity}>
          <Text
            style={styles.roleTitle}
            numberOfLines={1}
            testID={`preset-role-name-${row.id}`}
            accessibilityLabel={friendlyName}
          >
            {friendlyName}
          </Text>
        </View>
        <View style={styles.modelCell}>
          <SelectField
            field={false}
            size="sm"
            label={friendlyName}
            value={row.model}
            selectedDisplay={{ label: displayLabel }}
            options={options}
            onChange={(nextModel) =>
              onChange({
                ...row,
                model: nextModel,
                thinking: nextModel === row.model ? row.thinking : "",
              })
            }
            disabled={disabled}
            searchable
            placeholder={t("settings.providers.presets.model")}
            emptyText={t("providerControl.noModels")}
            testID={`preset-role-model-${row.id}`}
          />
          {compact ? <View style={styles.compactThinking}>{roleThinking}</View> : null}
        </View>
        {!compact ? <View style={styles.effortCell}>{roleThinking}</View> : null}
        <View style={styles.removeSpace}>
          {!disabled ? (
            <Button
              variant="ghost"
              size="sm"
              leftIcon={Trash2}
              accessibilityLabel={t("providerControl.removeRole")}
              onPress={() => onRemove(row)}
              testID={`preset-role-remove-${row.id}`}
            />
          ) : null}
        </View>
      </View>
      {supportsFallbacks ? (
        <FallbackChainEditor
          roleId={roleId}
          items={fallbacks}
          choices={fallbackChoices}
          inherited={inheritedFallbacks}
          catalogIndex={catalogIndex}
          disabled={disabled}
          compact={compact}
          createId={createFallbackId}
          onChange={onChangeFallbacks}
        />
      ) : null}
    </View>
  );
});

const FallbackChainEditor = memo(function FallbackChainEditor({
  roleId,
  items,
  choices,
  inherited,
  catalogIndex,
  disabled,
  compact,
  createId,
  onChange,
}: {
  roleId: string;
  items: readonly PresetFallbackItem[];
  choices: readonly RoleModelChoice[];
  inherited: readonly string[];
  catalogIndex: ModelCatalogIndex;
  disabled: boolean;
  compact: boolean;
  createId: () => string;
  onChange: (roleId: string, items: PresetFallbackItem[]) => void;
}) {
  const { t } = useTranslation();

  const selectOptions = useMemo(
    () =>
      choices.map((choice) => ({
        id: choice.id,
        value: choice.id,
        label: choice.label,
        description:
          choice.id.endsWith("/*") && choice.vendor
            ? t("providerControl.wildcardProvider", { provider: choice.vendor })
            : choice.description,
      })),
    [choices, t],
  );

  if (disabled && items.length === 0 && inherited.length === 0) {
    return null;
  }

  return (
    <View style={styles.fallbackSection} testID={`preset-fallbacks-${roleId}`}>
      {items.length === 0 && inherited.length > 0 ? (
        <Text style={styles.inheritedFallbackText} numberOfLines={1}>
          {t("providerControl.inheritedFallbacks", { model: inherited.join(" → ") })}
        </Text>
      ) : null}
      {items.map((item, index) => {
        const matchedChoice = choices.find((choice) => choice.id === item.model);
        const itemOptions = matchedChoice
          ? selectOptions
          : [
              ...selectOptions,
              {
                id: item.model,
                value: item.model,
                ...presetModelDisplay(item.model, catalogIndex),
              },
            ];
        const display = matchedChoice
          ? { label: matchedChoice.label, description: matchedChoice.description }
          : presetModelDisplay(item.model, catalogIndex);
        const itemEfforts = [...(matchedChoice?.thinkingOptions ?? [])];
        if (item.thinking && !itemEfforts.some((opt) => opt.id === item.thinking)) {
          itemEfforts.push({ id: item.thinking, label: item.thinking });
        }
        const canEditThinking =
          !item.model.startsWith("@") && !item.model.endsWith("/*") && itemEfforts.length > 0;

        const thinkingField = canEditThinking ? (
          <SelectField
            field={false}
            size="sm"
            label={t("settings.providers.presets.thinking")}
            value={item.thinking}
            selectedDisplay={{
              label: item.thinking
                ? formatThinkingOptionLabel({ id: item.thinking }, t)
                : t("providerControl.roleDefaultShort"),
            }}
            options={[
              { id: "", value: "", label: t("providerControl.roleDefaultThinking") },
              ...itemEfforts.map((opt) => ({
                id: opt.id,
                value: opt.id,
                label: formatThinkingOptionLabel(opt, t),
              })),
            ]}
            onChange={(nextThinking) => {
              const next = [...items];
              next[index] = { ...item, thinking: nextThinking };
              onChange(roleId, next);
            }}
            disabled={disabled}
            placeholder={t("settings.providers.presets.thinking")}
            emptyText={t("providerControl.roleDefaultThinking")}
            testID={`preset-fallback-thinking-${roleId}-${index}`}
          />
        ) : null;

        return (
          <View
            key={item.id}
            style={styles.fallbackRow}
            testID={`preset-fallback-row-${roleId}-${index}`}
          >
            <View style={styles.fallbackLead}>
              <Text style={styles.fallbackIndex}>
                {index === 0 ? `${t("providerControl.fallbacks")} 1` : `${index + 1}`}
              </Text>
            </View>
            <View style={styles.modelCell}>
              <SelectField
                field={false}
                size="sm"
                label={t("providerControl.fallbacks")}
                value={item.model}
                selectedDisplay={{ label: display.label }}
                options={itemOptions}
                onChange={(nextModel) => {
                  const next = [...items];
                  next[index] = {
                    ...item,
                    model: nextModel,
                    thinking: nextModel === item.model ? item.thinking : "",
                  };
                  onChange(roleId, next);
                }}
                disabled={disabled}
                searchable
                placeholder={t("settings.providers.presets.model")}
                emptyText={t("providerControl.noModels")}
                testID={`preset-fallback-model-${roleId}-${index}`}
              />
              {compact && thinkingField ? (
                <View style={styles.compactThinking}>{thinkingField}</View>
              ) : null}
            </View>
            {!compact ? <View style={styles.effortCell}>{thinkingField}</View> : null}
            {!disabled ? (
              <View style={styles.fallbackActions}>
                {items.length > 1 ? (
                  <>
                    <Button
                      variant="ghost"
                      size="sm"
                      leftIcon={ArrowUp}
                      disabled={index === 0}
                      accessibilityLabel={t("providerControl.moveFallbackUp")}
                      onPress={() => {
                        if (index === 0) return;
                        const next = [...items];
                        next[index - 1] = items[index]!;
                        next[index] = items[index - 1]!;
                        onChange(roleId, next);
                      }}
                      testID={`preset-fallback-up-${roleId}-${index}`}
                    />
                    <Button
                      variant="ghost"
                      size="sm"
                      leftIcon={ArrowDown}
                      disabled={index === items.length - 1}
                      accessibilityLabel={t("providerControl.moveFallbackDown")}
                      onPress={() => {
                        if (index >= items.length - 1) return;
                        const next = [...items];
                        next[index + 1] = items[index]!;
                        next[index] = items[index + 1]!;
                        onChange(roleId, next);
                      }}
                      testID={`preset-fallback-down-${roleId}-${index}`}
                    />
                  </>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={Trash2}
                  accessibilityLabel={t("providerControl.removeFallback")}
                  onPress={() => {
                    onChange(
                      roleId,
                      items.filter((_, idx) => idx !== index),
                    );
                  }}
                  testID={`preset-fallback-remove-${roleId}-${index}`}
                />
              </View>
            ) : (
              <View style={styles.removeSpace} />
            )}
          </View>
        );
      })}
      {!disabled && selectOptions.length > 0 ? (
        <View style={styles.addFallbackWrap}>
          <SelectField
            field={false}
            size="sm"
            label={t("providerControl.addFallback")}
            value={null}
            selectedDisplay={null}
            options={selectOptions}
            onChange={(nextModel) => {
              onChange(roleId, [...items, { id: createId(), model: nextModel, thinking: "" }]);
            }}
            searchable
            placeholder={t("providerControl.addFallback")}
            emptyText={t("providerControl.noModels")}
            testID={`preset-add-fallback-${roleId}`}
          />
        </View>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create((theme) => ({
  form: {
    gap: theme.spacing[4],
  },
  fieldGroup: {
    gap: theme.spacing[2],
  },
  sectionGroup: {
    gap: theme.spacing[2],
  },
  label: {
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  bannerText: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  input: {
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    color: theme.colors.foreground,
    backgroundColor: theme.colors.surface2,
    fontSize: theme.fontSize.base,
  },
  inputReadOnly: {
    opacity: 0.65,
  },
  table: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.lg,
    overflow: "hidden",
  },
  tableHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
  },
  roleGroup: {
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  roleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
  },
  roleHeading: {
    width: 112,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  roleIdentity: {
    width: 112,
    justifyContent: "center",
  },
  roleTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  modelHeading: {
    flex: 1,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  effortHeading: {
    width: 116,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  modelCell: {
    flex: 1,
    minWidth: 0,
    gap: theme.spacing[2],
  },
  compactThinking: {
    flexDirection: "row",
    alignItems: "center",
  },
  effortCell: {
    width: 116,
  },
  removeSpace: {
    width: 32,
    alignItems: "flex-end",
  },
  fallbackSection: {
    paddingHorizontal: theme.spacing[3],
    paddingBottom: theme.spacing[2],
    gap: theme.spacing[2],
  },
  inheritedFallbackText: {
    paddingLeft: 120,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  fallbackRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  fallbackLead: {
    width: 112,
    paddingLeft: theme.spacing[2],
  },
  fallbackIndex: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  fallbackActions: {
    flexDirection: "row",
    alignItems: "center",
  },
  addFallbackWrap: {
    paddingLeft: 120,
    maxWidth: 320,
  },
  actions: {
    flexDirection: "row",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
    flex: 1,
  },
  footer: {
    alignSelf: "stretch",
  },
  error: {
    color: theme.colors.destructive,
    fontSize: theme.fontSize.sm,
  },
}));
