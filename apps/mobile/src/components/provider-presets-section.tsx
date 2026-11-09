import type { AgentModelDefinition } from "@ait/protocol/agent-types";
import { SelectField } from "@/components/ui/select-field";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Pencil, Plus, Trash2 } from "lucide-react-native";
import type {
  ProviderPreset,
  ProviderPresetEntry,
  ProviderPresetDescriptor,
} from "@ait/protocol/messages";
import {
  AdaptiveModalSheet,
  AdaptiveTextInput,
  type SheetHeader,
} from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { useToast } from "@/contexts/toast-context";
import { useQueryClient } from "@tanstack/react-query";
import {
  invalidatePresetCaches,
  providerPresetsQueryKey,
  useProviderPresets,
  type ProviderPresetsSnapshot,
} from "@/data/provider-presets";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { toErrorMessage } from "@/utils/error-messages";

const PRESET_SNAP_POINTS = ["55%", "88%"];

/**
 * One preset editor for every provider.
 *
 * The store descriptor and each listed entry's ownership decide which controls are enabled, so a native store (OMP) and an
 * Ait-managed store look identical here. Creating, editing and deleting always go through the
 * same daemon request, and the difference in storage never reaches this component.
 */
export function ProviderPresetsSection({
  provider,
  serverId,
  cwd,
  models = [],
}: {
  provider: string;
  serverId: string;
  cwd?: string;
  models?: readonly AgentModelDefinition[];
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const queryClient = useQueryClient();
  const client = useHostRuntimeClient(serverId);
  // The editor reads the same cache entry as every picker, so a write invalidates one entry and
  // all readers refetch. Mutation preflights publish fresh ownership back to this same cache.
  const { snapshot, isLoading, error } = useProviderPresets({
    serverId,
    provider,
    cwd,
  });
  const descriptor = snapshot?.descriptor ?? null;
  const presets = snapshot?.presets ?? [];
  const [editing, setEditing] = useState<ProviderPreset | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  const ready = snapshot !== null && !isLoading && error === null;
  const canSave = ready && descriptor?.canCreate === true;
  const editingAllowed =
    ready &&
    (editing?.name
      ? descriptor?.canEdit === true &&
        writable(presets.find((preset) => preset.name === editing.name))
      : descriptor?.canCreate === true);

  const checkLatest = useCallback(
    async (name: string, creating: boolean) => {
      if (!client || !ready) throw new Error(t("providerControl.presetReadOnly"));
      const key = providerPresetsQueryKey(serverId, provider, cwd);
      type Store = { snapshot: ProviderPresetsSnapshot; error: string | null };
      let listed: { presets: ProviderPresetEntry[]; error?: string | null };
      try {
        listed = await client.listProviderPresets(provider, cwd);
        queryClient.setQueryData<Store>(key, (current) =>
          current
            ? {
                snapshot: { ...current.snapshot, presets: listed.presets },
                error: listed.error ?? null,
              }
            : current,
        );
      } catch (cause) {
        queryClient.setQueryData<Store>(key, (current) =>
          current ? { ...current, error: toErrorMessage(cause) } : current,
        );
        throw cause;
      }
      if (listed.error) throw new Error(listed.error);
      const current = listed.presets.find((preset) => preset.name === name);
      if (creating ? current !== undefined : !writable(current)) {
        throw new Error(t("providerControl.presetReadOnly"));
      }
    },
    [client, ready, t, provider, cwd, queryClient, serverId],
  );

  /**
   * Drop dependent caches.
   *
   * The invalidation also refetches this section's own entry. A rejected invalidation must not
   * hide the write that already succeeded, so the failure only warns.
   */
  const refreshAfterWrite = useCallback(async () => {
    try {
      await invalidatePresetCaches(queryClient, serverId, provider);
    } catch (cause) {
      console.warn("[presets] invalidate dependent caches failed", cause);
    }
  }, [provider, queryClient, serverId]);

  const handleSave = useCallback(
    async (preset: ProviderPreset) => {
      if (!client || !editingAllowed) throw new Error(t("providerControl.presetReadOnly"));
      await checkLatest(editing?.name || preset.name, !editing?.name);
      // The list was read for `cwd`, so the write must use the same directory: a native store
      // would otherwise save into the daemon's own directory and miss project-scoped roles.
      const result = await client.saveProviderPreset(
        provider,
        preset,
        cwd,
        editing?.name ? { original: editing.name } : {},
      );
      if (result.error) throw new Error(result.error);
      await refreshAfterWrite();
    },
    [client, cwd, editing?.name, editingAllowed, checkLatest, refreshAfterWrite, provider, t],
  );

  const handleDelete = useCallback(
    async (name: string) => {
      if (
        !client ||
        !ready ||
        !descriptor?.canDelete ||
        !writable(presets.find((preset) => preset.name === name))
      )
        return;
      setDeleting(name);
      try {
        await checkLatest(name, false);
        const result = await client.deleteProviderPreset(provider, name, cwd);
        if (result.error) {
          toast.error(result.error);
          return;
        }
        await refreshAfterWrite();
      } catch (cause) {
        toast.error(toErrorMessage(cause));
      } finally {
        setDeleting((current) => (current === name ? null : current));
      }
    },
    [
      client,
      cwd,
      ready,
      descriptor?.canDelete,
      presets,
      checkLatest,
      refreshAfterWrite,
      provider,
      toast,
    ],
  );

  return (
    <View style={styles.section} testID="provider-presets-section">
      <View style={styles.header}>
        <Text style={styles.title}>{t("settings.providers.presets.title")}</Text>
        {canSave ? (
          <Button
            variant="ghost"
            size="sm"
            leftIcon={Plus}
            onPress={() => setEditing({ name: "", model: "" })}
            testID="provider-presets-add"
          >
            {t("settings.providers.presets.create")}
          </Button>
        ) : null}
      </View>
      <Text style={styles.muted} testID="provider-presets-scope">
        {descriptor?.backend === "native"
          ? cwd
            ? t("providerControl.scopeProject", { cwd })
            : t("providerControl.scopeDefault")
          : t("providerControl.scopeManaged")}
      </Text>
      {descriptor?.backend === "native" ? (
        <Text style={styles.muted}>{t("providerControl.scopeOverride")}</Text>
      ) : null}
      {descriptor?.reason ? (
        <Text style={styles.muted} testID="provider-presets-reason">
          {descriptor.reason}
        </Text>
      ) : null}
      {error ? <Text style={styles.muted}>{error}</Text> : null}
      {isLoading ? (
        <Text style={styles.muted}>{t("settings.providers.presets.loading")}</Text>
      ) : null}
      {!isLoading && snapshot && presets.length === 0 ? (
        <View style={styles.empty} testID="provider-presets-empty">
          <Text style={styles.muted}>{t("settings.providers.presets.empty")}</Text>
          {canSave ? (
            <Button
              variant="default"
              size="sm"
              leftIcon={Plus}
              onPress={() => setEditing({ name: "", model: "" })}
              testID="provider-presets-empty-create"
            >
              {t("settings.providers.presets.create")}
            </Button>
          ) : null}
        </View>
      ) : null}
      {presets.length > 0 ? (
        <View style={styles.card}>
          {presets.map((preset) => (
            <View key={preset.name} style={styles.row} testID={`provider-preset-${preset.name}`}>
              <View style={styles.rowText}>
                <Text style={styles.rowTitle}>{preset.name}</Text>
                <Text style={styles.muted}>{preset.model}</Text>
                {descriptor?.backend === "native" && preset.source ? (
                  <Text style={styles.muted}>
                    {preset.source === "project"
                      ? t("providerControl.scopeProject", { cwd: cwd ?? "" })
                      : t("providerControl.scopeGlobal")}
                  </Text>
                ) : null}
                {!ready || !writable(preset) ? (
                  <Text style={styles.muted}>{t("providerControl.presetReadOnly")}</Text>
                ) : null}
              </View>
              {descriptor?.canEdit ? (
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={Pencil}
                  disabled={!ready || !writable(preset)}
                  onPress={() => {
                    if (ready && writable(preset)) setEditing(preset);
                  }}
                  accessibilityLabel={t("settings.providers.presets.edit", { name: preset.name })}
                />
              ) : null}
              {descriptor?.canDelete ? (
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={Trash2}
                  disabled={deleting === preset.name || !ready || !writable(preset)}
                  onPress={() => void handleDelete(preset.name)}
                  accessibilityLabel={t("settings.providers.presets.remove", { name: preset.name })}
                />
              ) : null}
            </View>
          ))}
        </View>
      ) : null}
      {editing ? (
        <PresetEditorSubSheet
          models={models}
          descriptor={descriptor}
          visible
          preset={editing}
          onClose={() => setEditing(null)}
          onSave={handleSave}
          canRename={descriptor?.canRename === true}
          canSave={editingAllowed}
          existingNames={presets.map((preset) => preset.name)}
        />
      ) : null}
    </View>
  );
}

/** Missing ownership on an older daemon or failed refresh must never grant write access. */
function writable(preset: ProviderPresetEntry | undefined): boolean {
  return preset?.editable === true && (preset.source === "global" || preset.source === "ait");
}

/**
 * Create or replace one preset.
 *
 * A store without rename keeps the original name; the field stays visible but read-only so the
 * limit is explained instead of failing after the save.
 */
function PresetEditorSubSheet({
  descriptor,
  models,
  visible,
  preset,
  onClose,
  onSave,
  canRename,
  canSave,
  existingNames,
}: {
  descriptor: ProviderPresetDescriptor | null;
  models: readonly AgentModelDefinition[];
  visible: boolean;
  preset: ProviderPreset;
  onClose: () => void;
  onSave: (preset: ProviderPreset) => Promise<void>;
  canRename: boolean;
  canSave: boolean;
  /** Names already stored, so a create cannot silently take one over. */
  existingNames: readonly string[];
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const [name, setName] = useState(preset.name);
  const [model, setModel] = useState(preset.model);
  const [thinking, setThinking] = useState(preset.thinkingOptionId ?? "");
  const [customModel, setCustomModel] = useState(
    !models.some((option) => option.id === preset.model),
  );
  const [customThinking, setCustomThinking] = useState(false);
  const [roles, setRoles] = useState(
    JSON.stringify(
      Object.fromEntries(
        Object.entries(preset.featureValues ?? {}).filter(([key]) => key.startsWith("modelRole.")),
      ),
      null,
      2,
    ),
  );
  const showCustomModel = customModel || !models.some((entry) => entry.id === model);
  const thinkingOptions = models.find((option) => option.id === model)?.thinkingOptions ?? [];
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isNew = preset.name.length === 0;

  useEffect(() => {
    setName(preset.name);
    setModel(preset.model);
    setThinking(preset.thinkingOptionId ?? "");
    setRoles(
      JSON.stringify(
        Object.fromEntries(
          Object.entries(preset.featureValues ?? {}).filter(([key]) =>
            key.startsWith("modelRole."),
          ),
        ),
        null,
        2,
      ),
    );
    setCustomModel(false);
    setCustomThinking(false);
    setError(null);
  }, [preset]);

  const trimmedName = name.trim();
  const trimmedModel = model.trim();
  // Creating under an existing name is an upsert in the store, which would replace that preset's
  // roles and thinking level with whatever this form carries. A create therefore stops here; an
  // edit already addresses its record through `original`.
  const takenName =
    isNew && existingNames.some((existing) => existing === trimmedName)
      ? t("settings.providers.presets.nameTaken", { name: trimmedName })
      : null;
  const canSubmit =
    canSave &&
    trimmedName.length > 0 &&
    trimmedModel.length > 0 &&
    takenName === null &&
    (canRename || isNew || trimmedName === preset.name);

  const handleSubmit = useCallback(() => {
    if (!canSubmit) return;
    let roleValues: Record<string, string>;
    try {
      const parsed: unknown = JSON.parse(roles);
      if (
        !parsed ||
        typeof parsed !== "object" ||
        Array.isArray(parsed) ||
        Object.entries(parsed).some(
          ([key, value]) =>
            !key.startsWith("modelRole.") || typeof value !== "string" || value.trim().length === 0,
        )
      )
        throw new Error(t("providerControl.rolesInvalid"));
      roleValues = parsed as Record<string, string>;
    } catch {
      setError(t("providerControl.rolesInvalid"));
      return;
    }
    setSaving(true);
    setError(null);
    const next: ProviderPreset = {
      name: trimmedName,
      model: trimmedModel,
      ...(thinking.trim() ? { thinkingOptionId: thinking.trim() } : {}),
      featureValues: {
        ...Object.fromEntries(
          Object.entries(preset.featureValues ?? {}).filter(
            ([key]) => !key.startsWith("modelRole."),
          ),
        ),
        ...roleValues,
      },
    };
    void onSave(next)
      .then(() => onClose())
      .catch((cause) => setError(toErrorMessage(cause)))
      .finally(() => setSaving(false));
  }, [
    canSubmit,
    onClose,
    onSave,
    preset.featureValues,
    roles,
    t,
    thinking,
    trimmedModel,
    trimmedName,
  ]);

  const header = useMemo<SheetHeader>(
    () => ({
      title: isNew
        ? t("settings.providers.presets.createTitle")
        : t("settings.providers.presets.editTitle", { name: preset.name }),
    }),
    [isNew, preset.name, t],
  );

  return (
    <AdaptiveModalSheet
      header={header}
      visible={visible}
      onClose={onClose}
      desktopMaxWidth={420}
      snapPoints={PRESET_SNAP_POINTS}
      testID="provider-preset-editor"
    >
      <View style={styles.formGroup}>
        <Text style={styles.formLabel}>{t("settings.providers.presets.name")}</Text>
        <AdaptiveTextInput
          initialValue={name}
          resetKey={`preset-name-${preset.name}`}
          onChangeText={setName}
          editable={canRename || isNew}
          placeholder={t("settings.providers.presets.namePlaceholder")}
          placeholderTextColor={theme.colors.foregroundMuted}
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="next"
          style={styles.formInput}
        />
        {takenName ? (
          <Text style={styles.errorText} testID="provider-preset-name-taken">
            {takenName}
          </Text>
        ) : null}
        {!canRename && !isNew ? (
          <Text style={styles.muted}>
            {descriptor?.reason ?? t("settings.providers.presets.noRename")}
          </Text>
        ) : null}
        <SelectField
          label={t("settings.providers.presets.model")}
          value={showCustomModel ? "__custom__" : model}
          selectedDisplay={null}
          options={[
            ...models.map((entry) => ({ id: entry.id, value: entry.id, label: entry.label })),
            { id: "__custom__", value: "__custom__", label: t("providerControl.customInput") },
          ]}
          onChange={(value) => {
            if (value === "__custom__") {
              setCustomModel(true);
              return;
            }
            setModel(value);
            setCustomModel(false);
            if (
              !models
                .find((entry) => entry.id === value)
                ?.thinkingOptions?.some((option) => option.id === thinking)
            )
              setThinking("");
          }}
          placeholder={t("settings.providers.presets.model")}
          emptyText={t("settings.providers.presets.empty")}
          searchable
          testID="provider-preset-model-options"
        />
        {showCustomModel ? (
          <AdaptiveTextInput
            initialValue={model}
            resetKey={`preset-model-${preset.name}-${customModel}`}
            onChangeText={setModel}
            placeholder={t("settings.providers.presets.modelPlaceholder")}
            placeholderTextColor={theme.colors.foregroundMuted}
            autoCapitalize="none"
            autoCorrect={false}
            returnKeyType="next"
            style={styles.formInput}
          />
        ) : null}
        <SelectField
          label={t("settings.providers.presets.thinking")}
          value={
            customThinking ||
            (thinking !== "" && !thinkingOptions.some((entry) => entry.id === thinking))
              ? "__custom__"
              : thinking
          }
          selectedDisplay={null}
          options={[
            { id: "", value: "", label: t("settings.providers.presets.thinkingPlaceholder") },
            ...thinkingOptions.map((entry) => ({
              id: entry.id,
              value: entry.id,
              label: entry.label,
            })),
            { id: "__custom__", value: "__custom__", label: t("providerControl.customInput") },
          ]}
          onChange={(value) => {
            if (value === "__custom__") {
              setCustomThinking(true);
              return;
            }
            setThinking(value);
            setCustomThinking(false);
          }}
          placeholder={t("settings.providers.presets.thinking")}
          emptyText={t("settings.providers.presets.empty")}
          testID="provider-preset-thinking-options"
        />
        {customThinking ||
        (thinking !== "" && !thinkingOptions.some((entry) => entry.id === thinking)) ? (
          <AdaptiveTextInput
            initialValue={thinking}
            resetKey={`preset-thinking-${preset.name}`}
            onChangeText={setThinking}
            placeholder={t("settings.providers.presets.thinkingPlaceholder")}
            placeholderTextColor={theme.colors.foregroundMuted}
            autoCapitalize="none"
            autoCorrect={false}
            returnKeyType="done"
            onSubmitEditing={handleSubmit}
            style={styles.formInput}
          />
        ) : null}
        <Text style={styles.formLabel}>{t("providerControl.modelRoles")}</Text>
        <Text style={styles.muted}>{t("providerControl.rolesEditable")}</Text>
        <AdaptiveTextInput
          initialValue={roles}
          resetKey={`preset-roles-${preset.name}`}
          onChangeText={setRoles}
          multiline
          autoCapitalize="none"
          autoCorrect={false}
          style={styles.formInput}
          testID="provider-preset-roles"
        />
        {!canSave ? (
          <Text style={styles.errorText}>{t("providerControl.presetReadOnly")}</Text>
        ) : null}
        {error ? <Text style={styles.errorText}>{error}</Text> : null}
        <View style={styles.formActions}>
          <Button variant="secondary" size="sm" onPress={onClose} disabled={saving}>
            {t("common.actions.cancel")}
          </Button>
          <Button
            variant="default"
            size="sm"
            onPress={handleSubmit}
            disabled={!canSubmit || saving}
            testID="provider-preset-save"
          >
            {saving ? t("settings.providers.presets.saving") : t("common.actions.save")}
          </Button>
        </View>
      </View>
    </AdaptiveModalSheet>
  );
}

const styles = StyleSheet.create((theme) => ({
  section: { marginBottom: theme.spacing[4], gap: theme.spacing[2] },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  title: {
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foregroundMuted,
  },
  muted: {
    fontSize: theme.fontSize.base,
    color: theme.colors.foregroundMuted,
  },
  empty: { paddingVertical: theme.spacing[6], alignItems: "center", gap: theme.spacing[3] },
  card: {
    backgroundColor: theme.colors.surface2,
    borderRadius: theme.borderRadius.lg,
    overflow: "hidden",
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
  },
  rowText: { flex: 1, minWidth: 0 },
  rowTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  formGroup: { gap: theme.spacing[3] },
  formLabel: {
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foreground,
  },
  formInput: {
    backgroundColor: theme.colors.surface2,
    borderRadius: theme.borderRadius.lg,
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[3],
    color: theme.colors.foreground,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  formActions: {
    flexDirection: "row",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
  },
  errorText: { fontSize: theme.fontSize.sm, color: theme.colors.destructive },
}));
