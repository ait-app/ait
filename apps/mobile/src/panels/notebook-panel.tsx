import { useCallback, useEffect, useRef, useState } from "react";
import { ActivityIndicator, Text, TextInput, View } from "react-native";
import invariant from "tiny-invariant";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { FileVersion } from "@ait/protocol/messages";
import { useAppSettings } from "@/hooks/use-settings";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useLiveFile } from "@/file-pane/live-file/hook";
import { usePaneContext, usePaneFocus } from "@/panels/pane-context";
import { usePublishPanelInstanceAttributes } from "@/panels/panel-instance-attributes";
import { definePanel } from "@/panels/panel-registry";
import { useWorkspaceDirectory } from "@/stores/session-store-hooks";
import { notebookPanelPresentation } from "@/panels/panel-presentations";
import { toErrorMessage } from "@/utils/error-messages";

export const WORKSPACE_NOTEBOOK_PATH = ".ait-notebook.md";

type NotebookStatus =
  | "loading"
  | "clean"
  | "unsaved"
  | "saving"
  | "saved"
  | "conflict"
  | "error"
  | "readonly";

const styles = StyleSheet.create((theme) => ({
  container: {
    flex: 1,
    backgroundColor: theme.colors.surface0,
  },
  header: {
    minHeight: 46,
    paddingHorizontal: 16,
    paddingVertical: 10,
    borderBottomWidth: 1,
    borderBottomColor: theme.colors.border,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 12,
  },
  title: {
    color: theme.colors.foreground,
    fontSize: 15,
    fontWeight: "600",
  },
  status: {
    color: theme.colors.foregroundMuted,
    fontSize: 12,
  },
  body: {
    flex: 1,
    padding: 16,
  },
  editor: {
    flex: 1,
    minHeight: 240,
    color: theme.colors.foreground,
    fontFamily: theme.fontFamily.mono,
    fontSize: 15,
    lineHeight: 23,
    textAlignVertical: "top",
  },
  hint: {
    color: theme.colors.foregroundMuted,
    lineHeight: 21,
  },
  center: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: 24,
  },
}));

function decodeNotebookContent(bytes: Uint8Array): string {
  const content = new TextDecoder().decode(bytes);
  return content.startsWith("\uFEFF") ? content.slice(1) : content;
}

function isReadOnlyError(error: unknown): boolean {
  const message = toErrorMessage(error);
  return /read.?only|permission|forbidden|denied/i.test(message);
}

function fileVersionKey(version: Extract<FileVersion, { status: "ready" }>): string {
  return `${version.modifiedAt}:${version.revision ?? ""}`;
}

function NotebookPanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target } = usePaneContext();
  const { isInteractive } = usePaneFocus();
  const {
    settings: { experimentalFeatures },
  } = useAppSettings();
  const client = useHostRuntimeClient(serverId);
  const workspaceDirectory = useWorkspaceDirectory(serverId, workspaceId);
  const [content, setContent] = useState("");
  const [persistedContent, setPersistedContent] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [status, setStatus] = useState<NotebookStatus>("loading");
  const versionRef = useRef<Extract<FileVersion, { status: "ready" }> | null>(null);
  const latestContentRef = useRef(content);
  const createAttemptedRef = useRef(false);
  const saveGenerationRef = useRef(0);
  const lastAppliedVersionKeyRef = useRef<string | null>(null);
  const invariantTarget = target;
  invariant(invariantTarget.kind === "notebook", "NotebookPanel requires notebook target");

  const enabled = experimentalFeatures && Boolean(client && workspaceDirectory);
  const liveFile = useLiveFile({
    client,
    cwd: workspaceDirectory,
    path: WORKSPACE_NOTEBOOK_PATH,
    enabled,
    liveUpdates: true,
  });
  const observation = liveFile.snapshot.observation;
  const dirty = loaded && content !== persistedContent;
  const canEdit = enabled && isInteractive && status !== "readonly" && status !== "conflict";

  usePublishPanelInstanceAttributes({
    modified: dirty,
    suspendPendingSave: useCallback(() => {
      saveGenerationRef.current += 1;
      return () => {
        saveGenerationRef.current += 1;
      };
    }, []),
  });

  useEffect(() => {
    latestContentRef.current = content;
  }, [content]);

  useEffect(() => {
    if (!enabled || !client || !workspaceDirectory || observation?.status !== "missing") {
      return;
    }
    if (createAttemptedRef.current) return;
    createAttemptedRef.current = true;
    setStatus("loading");
    void client
      .createFileEntry({
        cwd: workspaceDirectory,
        parentPath: ".",
        name: WORKSPACE_NOTEBOOK_PATH,
        kind: "file",
      })
      .then((result) => {
        if (!result.success) {
          setStatus(isReadOnlyError(result.error) ? "readonly" : "error");
          return;
        }
        liveFile.refresh();
      })
      .catch((error) => setStatus(isReadOnlyError(error) ? "readonly" : "error"));
  }, [client, enabled, liveFile, observation?.status, workspaceDirectory]);

  useEffect(() => {
    if (observation?.status !== "ready" || !liveFile.file) return;
    const nextContent = decodeNotebookContent(liveFile.file.bytes);
    const nextVersionKey = fileVersionKey(observation.version);
    if (loaded && dirty) return;
    if (loaded && lastAppliedVersionKeyRef.current === nextVersionKey) return;
    versionRef.current = observation.version;
    lastAppliedVersionKeyRef.current = nextVersionKey;
    setContent(nextContent);
    setPersistedContent(nextContent);
    setLoaded(true);
    setStatus("clean");
  }, [dirty, liveFile.file, loaded, observation]);

  useEffect(() => {
    if (!dirty || !canEdit || !client || !workspaceDirectory || !versionRef.current) return;
    const generation = saveGenerationRef.current;
    const timer = setTimeout(() => {
      const expectedVersion = versionRef.current;
      const nextContent = latestContentRef.current;
      if (!expectedVersion) return;
      setStatus("saving");
      void client
        .writeFile({
          cwd: workspaceDirectory,
          path: WORKSPACE_NOTEBOOK_PATH,
          content: nextContent,
          expectedModifiedAt: expectedVersion.modifiedAt,
          expectedRevision: expectedVersion.revision,
        })
        .then((result) => {
          if (generation !== saveGenerationRef.current) return;
          if (result.status === "written") {
            versionRef.current = {
              ...expectedVersion,
              size: result.size,
              modifiedAt: result.modifiedAt,
              revision: result.revision,
            };
            setPersistedContent(nextContent);
            setStatus(latestContentRef.current === nextContent ? "saved" : "unsaved");
            return;
          }
          if (result.status === "conflict") {
            setStatus("conflict");
            return;
          }
          setStatus(isReadOnlyError(result.error) ? "readonly" : "error");
        })
        .catch((error) => {
          if (generation === saveGenerationRef.current) {
            setStatus(isReadOnlyError(error) ? "readonly" : "error");
          }
        });
    }, 500);
    return () => clearTimeout(timer);
  }, [canEdit, client, dirty, workspaceDirectory]);

  const handleChange = useCallback(
    (nextContent: string) => {
      if (!canEdit) return;
      setContent(nextContent);
      setStatus("unsaved");
    },
    [canEdit],
  );

  if (!experimentalFeatures) {
    return (
      <View style={styles.center} testID="workspace-notebook-disabled">
        <Text style={styles.hint}>{t("panels.notebook.disabled")}</Text>
      </View>
    );
  }
  if (!workspaceDirectory) {
    return (
      <View style={styles.center} testID="workspace-notebook-directory-missing">
        <Text style={styles.hint}>{t("panels.file.directoryMissing")}</Text>
      </View>
    );
  }
  if (observation?.status === "error" || liveFile.error) {
    return (
      <View style={styles.center} testID="workspace-notebook-error">
        <Text style={styles.hint}>{t("panels.notebook.saveFailed")}</Text>
      </View>
    );
  }

  const statusLabel =
    status === "saving"
      ? t("panels.notebook.saving")
      : status === "saved" || status === "clean"
        ? t("panels.notebook.saved")
        : status === "conflict"
          ? t("panels.notebook.conflict")
          : status === "readonly"
            ? t("panels.notebook.readOnly")
            : status === "error"
              ? t("panels.notebook.saveFailed")
              : dirty
                ? t("panels.notebook.unsaved")
                : t("panels.notebook.loading");

  return (
    <View style={styles.container} testID="workspace-notebook-panel">
      <View style={styles.header}>
        <Text style={styles.title}>{t("panels.notebook.label")}</Text>
        {status === "loading" ? (
          <ActivityIndicator />
        ) : (
          <Text style={styles.status}>{statusLabel}</Text>
        )}
      </View>
      <View style={styles.body}>
        <TextInput
          testID="workspace-notebook-editor"
          style={styles.editor}
          value={content}
          onChangeText={handleChange}
          editable={canEdit}
          multiline
          placeholder={t("panels.notebook.placeholder")}
          placeholderTextColor="#8b8b8b"
          autoCorrect
          spellCheck
        />
      </View>
    </View>
  );
}

export const notebookPanelRegistration = definePanel("notebook", {
  component: NotebookPanel,
  presentation: notebookPanelPresentation,
});
