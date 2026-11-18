import type { AgentModelDefinition } from "@ait/protocol/agent-types";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { StyleSheet as RNStyleSheet, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Eye, Pencil, Plus, Trash2 } from "lucide-react-native";
import type { ProviderPreset, ProviderPresetEntry } from "@ait/protocol/messages";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { useQueryClient } from "@tanstack/react-query";
import { formatThinkingOptionLabel } from "@/agent-controls/labels";
import {
  invalidatePresetCaches,
  useProviderPresets,
  useProviderPresetFeatures,
} from "@/data/provider-presets";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { toErrorMessage } from "@/utils/error-messages";

import { ProviderPresetEditor } from "./provider-preset-editor";
import { buildModelCatalogIndex, presetModelLabel } from "./provider-preset-form";

/**
 * One preset editor for every provider.
 *
 * The store descriptor and each listed entry's ownership decide which controls are enabled, so a
 * native store (OMP) and an Ait-managed store share the same controls and daemon RPCs.
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
  const queryClient = useQueryClient();
  const client = useHostRuntimeClient(serverId);
  const { snapshot, isSupported, isLoading, error } = useProviderPresets({
    serverId,
    provider,
    cwd,
  });
  const descriptor = snapshot?.descriptor ?? null;
  const presets = snapshot?.presets ?? [];
  const catalogIndex = useMemo(() => buildModelCatalogIndex(models), [models]);
  const [editing, setEditing] = useState<ProviderPresetEntry | null>(null);
  const roleFeatures = useProviderPresetFeatures(serverId, provider, cwd, isSupported);
  const [deleteTarget, setDeleteTarget] = useState<ProviderPresetEntry | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

  const scopeGeneration = useRef(0);
  useEffect(() => {
    scopeGeneration.current += 1;
    setEditing(null);
    setDeleteTarget(null);
    setDeleteError(null);
    setNotice(null);
    setDeleting(null);
  }, [serverId, provider, cwd]);

  const ready = snapshot !== null && !isLoading && error === null;
  const canCreate = ready && descriptor?.canCreate === true;
  const editingAllowed =
    ready &&
    (editing?.name
      ? descriptor?.canEdit === true &&
        writable(presets.find((preset) => preset.name === editing.name))
      : descriptor?.canCreate === true);

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
      const result = await client.saveProviderPreset(
        provider,
        preset,
        cwd,
        editing?.name ? { original: editing.name } : {},
      );
      if (result.error) {
        await refreshAfterWrite();
        throw new Error(result.error);
      }
      await refreshAfterWrite();
    },
    [client, cwd, editing?.name, editingAllowed, refreshAfterWrite, provider, t],
  );

  const handleDelete = useCallback(
    async (name: string) => {
      if (
        !client ||
        !ready ||
        !descriptor?.canDelete ||
        !writable(presets.find((preset) => preset.name === name))
      ) {
        return;
      }
      const generation = scopeGeneration.current;
      const isStale = () => generation !== scopeGeneration.current;
      setDeleting(name);
      setDeleteError(null);
      try {
        const result = await client.deleteProviderPreset(provider, name, cwd);
        if (result.error) {
          await refreshAfterWrite();
          if (!isStale()) setDeleteError(result.error);
          return;
        }
        await refreshAfterWrite();
        if (isStale()) return;
        setDeleteTarget(null);
        setNotice(t("providerControl.presetDeleted", { name }));
      } catch (cause) {
        await refreshAfterWrite();
        if (!isStale()) setDeleteError(toErrorMessage(cause));
      } finally {
        if (!isStale()) setDeleting((current) => (current === name ? null : current));
      }
    },
    [client, cwd, ready, descriptor?.canDelete, presets, refreshAfterWrite, provider, t],
  );

  if (!isSupported) return null;

  return (
    <View style={styles.section} testID="provider-presets-section">
      <View style={styles.header}>
        <View style={styles.headerTitleRow}>
          <Text style={styles.title}>{t("settings.providers.presets.title")}</Text>
          {presets.length > 0 ? <Text style={styles.count}>{presets.length}</Text> : null}
        </View>
        {canCreate && presets.length > 0 ? (
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
      {notice ? (
        <Text
          accessibilityLiveRegion="polite"
          testID="provider-presets-notice"
          style={styles.muted}
        >
          {notice}
        </Text>
      ) : null}
      {roleFeatures.error ? (
        <Text style={styles.errorText}>{t("providerControl.roleChoicesFailed")}</Text>
      ) : null}
      {error ? <Text style={styles.errorText}>{error}</Text> : null}
      {isLoading && snapshot === null ? (
        <Text style={styles.muted}>{t("settings.providers.presets.loading")}</Text>
      ) : null}
      {!isLoading && snapshot && presets.length === 0 ? (
        <View style={styles.emptyCard} testID="provider-presets-empty">
          <Text style={styles.muted}>{t("settings.providers.presets.empty")}</Text>
          {canCreate ? (
            <Button
              variant="secondary"
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
          {presets.map((preset, index) => {
            const modelSummary = presetModelLabel(preset.model, catalogIndex);
            const thinkingLabel = preset.thinkingOptionId
              ? formatThinkingOptionLabel({ id: preset.thinkingOptionId }, t)
              : null;
            const summary = thinkingLabel ? `${modelSummary} · ${thinkingLabel}` : modelSummary;
            const isProject = preset.source === "project";
            const isUnconfirmed = !isProject && (!ready || !writable(preset));
            return (
              <View
                key={preset.name}
                style={[styles.row, index < presets.length - 1 ? styles.rowDivider : null]}
                testID={`provider-preset-${preset.name}`}
              >
                <View style={styles.rowText}>
                  <View style={styles.rowHeader}>
                    <Text style={styles.rowTitle} numberOfLines={1}>
                      {preset.name}
                    </Text>
                    {isProject ? (
                      <Text style={styles.badge}>{t("providerControl.sourceProject")}</Text>
                    ) : null}
                  </View>
                  <Text style={styles.rowSubtitle} numberOfLines={1}>
                    {summary}
                  </Text>
                  {isUnconfirmed ? (
                    <Text style={styles.rowSubtitle}>
                      {t(
                        error
                          ? "providerControl.presetLoadFailed"
                          : isLoading
                            ? "providerControl.presetChecking"
                            : "providerControl.presetOwnershipUnknown",
                      )}
                    </Text>
                  ) : null}
                </View>
                <Button
                  variant="ghost"
                  size="sm"
                  leftIcon={ready && writable(preset) && descriptor?.canEdit ? Pencil : Eye}
                  onPress={() => setEditing(preset)}
                  accessibilityLabel={t(
                    ready && writable(preset) && descriptor?.canEdit
                      ? "settings.providers.presets.edit"
                      : "providerControl.viewPreset",
                    { name: preset.name },
                  )}
                />
                {descriptor?.canDelete ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    leftIcon={Trash2}
                    loading={deleting === preset.name}
                    disabled={deleting !== null || !ready || !writable(preset)}
                    onPress={() => {
                      setDeleteError(null);
                      setDeleteTarget(preset);
                    }}
                    accessibilityLabel={t("settings.providers.presets.remove", {
                      name: preset.name,
                    })}
                  />
                ) : null}
              </View>
            );
          })}
        </View>
      ) : null}
      {editing ? (
        <ProviderPresetEditor
          key={editing.name}
          models={models}
          features={roleFeatures.data ?? []}
          preset={editing}
          canRename={descriptor?.canRename === true}
          native={descriptor?.backend === "native"}
          canSave={editingAllowed && !roleFeatures.isError && roleFeatures.data !== undefined}
          readOnlyReason={
            descriptor?.reason ??
            t(
              presets.find((item) => item.name === editing.name)?.source === "project"
                ? "providerControl.projectPresetReadOnly"
                : "providerControl.presetOwnershipUnknown",
            )
          }
          readOnly={
            Boolean(editing.name) &&
            (!writable(presets.find((preset) => preset.name === editing.name)) ||
              !descriptor?.canEdit)
          }
          existingNames={presets.map((preset) => preset.name)}
          onSave={handleSave}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {deleteTarget ? (
        <AdaptiveModalSheet
          visible
          header={{ title: t("providerControl.deletePresetTitle", { name: deleteTarget.name }) }}
          onClose={() => {
            if (!deleting) setDeleteTarget(null);
          }}
          desktopMaxWidth={420}
          snapPoints={["40%"]}
          testID="provider-preset-delete-confirmation"
        >
          <View style={styles.details}>
            <Text style={styles.rowTitle}>{deleteTarget.name}</Text>
            <Text style={styles.muted}>{t("providerControl.deletePresetScope", { provider })}</Text>
            <Text style={styles.muted}>{t("providerControl.deletePresetWarning")}</Text>
            {deleteError ? (
              <Text accessibilityRole="alert" style={styles.errorText}>
                {deleteError}
              </Text>
            ) : null}
            <View style={styles.formActions}>
              <Button
                variant="secondary"
                size="sm"
                disabled={deleting !== null}
                onPress={() => setDeleteTarget(null)}
              >
                {t("common.actions.cancel")}
              </Button>
              <Button
                variant="destructive"
                size="sm"
                loading={deleting !== null}
                disabled={
                  deleting !== null ||
                  !ready ||
                  !writable(presets.find((preset) => preset.name === deleteTarget.name))
                }
                onPress={() => void handleDelete(deleteTarget.name)}
                testID="provider-preset-confirm-delete"
              >
                {t(deleting ? "providerControl.deletingPreset" : "providerControl.deletePreset")}
              </Button>
            </View>
          </View>
        </AdaptiveModalSheet>
      ) : null}
    </View>
  );
}

/** Missing ownership on an older daemon or failed refresh must never grant write access. */
function writable(preset: ProviderPresetEntry | undefined): boolean {
  return preset?.editable === true && (preset.source === "global" || preset.source === "ait");
}

const styles = StyleSheet.create((theme) => ({
  details: {
    gap: theme.spacing[3],
  },
  section: {
    marginBottom: theme.spacing[4],
    gap: theme.spacing[2],
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[2],
  },
  headerTitleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  title: {
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foregroundMuted,
  },
  count: {
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
    color: theme.colors.foregroundMuted,
  },
  muted: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
  emptyCard: {
    backgroundColor: theme.colors.surface2,
    borderRadius: theme.borderRadius.lg,
    borderWidth: 1,
    borderColor: theme.colors.border,
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[4],
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: theme.spacing[3],
  },
  card: {
    backgroundColor: theme.colors.surface2,
    borderRadius: theme.borderRadius.lg,
    borderWidth: 1,
    borderColor: theme.colors.border,
    overflow: "hidden",
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    paddingVertical: theme.spacing[2],
  },
  rowDivider: {
    borderBottomWidth: RNStyleSheet.hairlineWidth,
    borderBottomColor: theme.colors.border,
  },
  rowText: {
    flex: 1,
    minWidth: 0,
    gap: 2,
  },
  rowHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
  },
  rowTitle: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    flexShrink: 1,
  },
  rowSubtitle: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  badge: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
  formActions: {
    flexDirection: "row",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
  },
  errorText: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.destructive,
  },
}));
