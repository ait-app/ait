import type { AgentModelDefinition } from "@ait/protocol/agent-types";
import { useCallback, useEffect, useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Eye, Pencil, Plus, Trash2 } from "lucide-react-native";
import type { ProviderPreset, ProviderPresetEntry } from "@ait/protocol/messages";
import { AdaptiveModalSheet } from "@/components/adaptive-modal-sheet";
import { Button } from "@/components/ui/button";
import { useQueryClient } from "@tanstack/react-query";
import {
  invalidatePresetCaches,
  providerPresetsQueryKey,
  useProviderPresets,
  useProviderPresetFeatures,
  type ProviderPresetsSnapshot,
} from "@/data/provider-presets";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { toErrorMessage } from "@/utils/error-messages";

import { ProviderPresetEditor } from "./provider-preset-editor";
import { presetModelLabel } from "./provider-preset-form";

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
  const queryClient = useQueryClient();
  const client = useHostRuntimeClient(serverId);
  // The editor reads the same cache entry as every picker, so a write invalidates one entry and
  // all readers refetch. Mutation preflights publish fresh ownership back to this same cache.
  const { snapshot, isSupported, isLoading, error } = useProviderPresets({
    serverId,
    provider,
    cwd,
  });
  const descriptor = snapshot?.descriptor ?? null;
  const presets = snapshot?.presets ?? [];
  const [editing, setEditing] = useState<ProviderPresetEntry | null>(null);
  const roleFeatures = useProviderPresetFeatures(serverId, provider, cwd, isSupported);
  const [deleteTarget, setDeleteTarget] = useState<ProviderPresetEntry | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  // Bumped on every scope change so a delete still in flight from the previous scope can neither
  // keep this scope's buttons disabled nor report its result here.
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
      const generation = scopeGeneration.current;
      const isStale = () => generation !== scopeGeneration.current;
      setDeleting(name);
      setDeleteError(null);
      try {
        await checkLatest(name, false);
        const result = await client.deleteProviderPreset(provider, name, cwd);
        if (result.error) {
          if (!isStale()) setDeleteError(result.error);
          return;
        }
        await refreshAfterWrite();
        if (isStale()) return;
        setDeleteTarget(null);
        setNotice(t("providerControl.presetDeleted", { name }));
      } catch (cause) {
        if (!isStale()) setDeleteError(toErrorMessage(cause));
      } finally {
        if (!isStale()) setDeleting((current) => (current === name ? null : current));
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
      t,
    ],
  );

  if (!isSupported) return null;

  return (
    <View style={styles.section} testID="provider-presets-section">
      <View style={styles.header}>
        <Text style={styles.title}>{t("settings.providers.presets.title")}</Text>
        {canSave && presets.length > 0 ? (
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
      <Text style={styles.rowTitle} testID="provider-presets-scope" numberOfLines={1} selectable>
        {cwd || t("providerControl.sourceGlobal")}
      </Text>
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
        <Text style={styles.muted}>{t("providerControl.roleChoicesFailed")}</Text>
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
                <Text style={styles.muted}>{presetModelLabel(preset.model, models)}</Text>
                <Text style={styles.badge}>
                  {t(
                    preset.source === "project"
                      ? "providerControl.sourceProject"
                      : preset.source === "global" || preset.source === "ait"
                        ? "providerControl.sourceGlobal"
                        : "providerControl.sourceUnknown",
                  )}
                </Text>
                {(!ready || !writable(preset)) && preset.source !== "project" ? (
                  <Text style={styles.muted}>
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
                  accessibilityLabel={t("settings.providers.presets.remove", { name: preset.name })}
                />
              ) : null}
            </View>
          ))}
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
          canSave={editingAllowed && roleFeatures.isSuccess && !roleFeatures.isFetching}
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
  details: { gap: theme.spacing[3] },
  badge: { fontSize: theme.fontSize.sm, color: theme.colors.foregroundMuted },
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
  formActions: {
    flexDirection: "row",
    justifyContent: "flex-end",
    gap: theme.spacing[2],
  },
  errorText: { fontSize: theme.fontSize.sm, color: theme.colors.destructive },
}));
