import { useCallback, useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { Pencil, Plus, Trash2 } from "lucide-react-native";
import type { ProviderPreset, ProviderPresetDescriptor } from "@ait/protocol/messages";
import {
  AdaptiveModalSheet,
  AdaptiveTextInput,
  type SheetHeader,
} from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { useToast } from "@/contexts/toast-context";
import { useQueryClient } from "@tanstack/react-query";
import { invalidateProviderPresets } from "@/data/provider-presets";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { toErrorMessage } from "@/utils/error-messages";

const PRESET_SNAP_POINTS = ["55%", "88%"];

/**
 * One preset editor for every provider.
 *
 * The descriptor alone decides which controls are enabled, so a native store (OMP) and an
 * Ait-managed store look identical here. Creating, editing and deleting always go through the
 * same daemon request, and the difference in storage never reaches this component.
 */
export function ProviderPresetsSection({
  provider,
  serverId,
}: {
  provider: string;
  serverId: string;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const queryClient = useQueryClient();
  const client = useHostRuntimeClient(serverId);
  const [descriptor, setDescriptor] = useState<ProviderPresetDescriptor | null>(null);
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<ProviderPreset | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!client) return;
    setLoading(true);
    try {
      const [described, listed] = await Promise.all([
        client.describeProviderPresets(provider),
        client.listProviderPresets(provider),
      ]);
      setDescriptor(described.descriptor);
      setPresets(listed.presets);
      setError(listed.error ?? null);
    } catch (cause) {
      setError(toErrorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [client, provider]);

  useEffect(() => {
    void load();
  }, [load]);

  const canSave = descriptor?.canCreate === true || descriptor?.canEdit === true;

  const handleSave = useCallback(
    async (preset: ProviderPreset) => {
      if (!client) return;
      const result = await client.saveProviderPreset(provider, preset);
      if (result.error) throw new Error(result.error);
      // Every composer reads the same cached store, so the new preset must appear there too.
      invalidateProviderPresets(queryClient, serverId, provider);
      await load();
    },
    [client, load, provider, queryClient, serverId],
  );

  const handleDelete = useCallback(
    async (name: string) => {
      if (!client) return;
      setDeleting(name);
      try {
        const result = await client.deleteProviderPreset(provider, name);
        if (result.error) {
          toast.error(result.error);
          return;
        }
        invalidateProviderPresets(queryClient, serverId, provider);
        await load();
      } catch (cause) {
        toast.error(toErrorMessage(cause));
      } finally {
        setDeleting((current) => (current === name ? null : current));
      }
    },
    [client, load, provider, queryClient, serverId, toast],
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
      {descriptor?.reason ? (
        <Text style={styles.muted} testID="provider-presets-reason">
          {descriptor.reason}
        </Text>
      ) : null}
      {error ? <Text style={styles.muted}>{error}</Text> : null}
      {presets.length === 0 ? (
        <View style={styles.empty} testID="provider-presets-empty">
          <Text style={styles.muted}>
            {loading
              ? t("settings.providers.presets.loading")
              : t("settings.providers.presets.empty")}
          </Text>
          {canSave && !loading ? (
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
      ) : (
        <View style={styles.card}>
          {presets.map((preset) => (
            <View key={preset.name} style={styles.row} testID={`provider-preset-${preset.name}`}>
              <View style={styles.rowText}>
                <Text style={styles.rowTitle}>{preset.name}</Text>
                <Text style={styles.muted}>{preset.model}</Text>
              </View>
              {descriptor?.canEdit ? (
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={Pencil}
                  onPress={() => setEditing(preset)}
                  accessibilityLabel={t("settings.providers.presets.edit", { name: preset.name })}
                />
              ) : null}
              {descriptor?.canDelete ? (
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={Trash2}
                  disabled={deleting === preset.name}
                  onPress={() => void handleDelete(preset.name)}
                  accessibilityLabel={t("settings.providers.presets.remove", { name: preset.name })}
                />
              ) : null}
            </View>
          ))}
        </View>
      )}
      {editing ? (
        <PresetEditorSubSheet
          descriptor={descriptor}
          visible
          preset={editing}
          onClose={() => setEditing(null)}
          onSave={handleSave}
          canRename={descriptor?.canRename === true}
        />
      ) : null}
    </View>
  );
}

/**
 * Create or replace one preset.
 *
 * A store without rename keeps the original name; the field stays visible but read-only so the
 * limit is explained instead of failing after the save.
 */
function PresetEditorSubSheet({
  descriptor,
  visible,
  preset,
  onClose,
  onSave,
  canRename,
}: {
  descriptor: ProviderPresetDescriptor | null;
  visible: boolean;
  preset: ProviderPreset;
  onClose: () => void;
  onSave: (preset: ProviderPreset) => Promise<void>;
  canRename: boolean;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const [name, setName] = useState(preset.name);
  const [model, setModel] = useState(preset.model);
  const [thinking, setThinking] = useState(preset.thinkingOptionId ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isNew = preset.name.length === 0;

  useEffect(() => {
    setName(preset.name);
    setModel(preset.model);
    setThinking(preset.thinkingOptionId ?? "");
    setError(null);
  }, [preset]);

  const trimmedName = name.trim();
  const trimmedModel = model.trim();
  const canSubmit =
    trimmedName.length > 0 &&
    trimmedModel.length > 0 &&
    (canRename || isNew || trimmedName === preset.name);

  const handleSubmit = useCallback(() => {
    if (!canSubmit) return;
    setSaving(true);
    setError(null);
    const next: ProviderPreset = {
      name: trimmedName,
      model: trimmedModel,
      ...(thinking.trim() ? { thinkingOptionId: thinking.trim() } : {}),
      ...(preset.featureValues && Object.keys(preset.featureValues).length > 0
        ? { featureValues: preset.featureValues }
        : {}),
    };
    void onSave(next)
      .then(() => onClose())
      .catch((cause) => setError(toErrorMessage(cause)))
      .finally(() => setSaving(false));
  }, [canSubmit, onClose, onSave, preset.featureValues, thinking, trimmedModel, trimmedName]);

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
        {!canRename && !isNew ? (
          <Text style={styles.muted}>
            {descriptor?.reason ?? t("settings.providers.presets.noRename")}
          </Text>
        ) : null}
        <Text style={styles.formLabel}>{t("settings.providers.presets.model")}</Text>
        <AdaptiveTextInput
          initialValue={model}
          resetKey={`preset-model-${preset.name}`}
          onChangeText={setModel}
          placeholder={t("settings.providers.presets.modelPlaceholder")}
          placeholderTextColor={theme.colors.foregroundMuted}
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="next"
          style={styles.formInput}
        />
        <Text style={styles.formLabel}>{t("settings.providers.presets.thinking")}</Text>
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
