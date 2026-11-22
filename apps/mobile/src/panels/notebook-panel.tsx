import { useCallback, useEffect, useRef, useState } from "react";
import { ActivityIndicator, Pressable, Text, TextInput, View } from "react-native";
import invariant from "tiny-invariant";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { useAppSettings } from "@/hooks/use-settings";
import { useHostFeature } from "@/runtime/host-features";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { usePaneContext, usePaneFocus } from "@/panels/pane-context";
import { usePublishPanelInstanceAttributes } from "@/panels/panel-instance-attributes";
import { definePanel } from "@/panels/panel-registry";
import { useWorkspaceDirectory } from "@/stores/session-store-hooks";
import { notebookPanelPresentation } from "@/panels/panel-presentations";
import { toErrorMessage } from "@/utils/error-messages";

/** The file earlier versions kept the notes in; its text is imported once and the file is kept. */
const LEGACY_NOTEBOOK_PATH = ".ait-notebook.md";
// The host sends no change event for the notepad.
const POLL_INTERVAL_MS = 3_000;
const SAVE_DEBOUNCE_MS = 500;

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
    flexShrink: 1,
  },
  statusRow: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  reload: {
    color: theme.colors.accent,
    fontSize: 12,
    fontWeight: "600",
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

type NotepadClient = NonNullable<ReturnType<typeof useHostRuntimeClient>>;

/** Text of the legacy notebook file, or null when there is nothing to import. */
async function readLegacyNotebook(client: NotepadClient, cwd: string): Promise<string | null> {
  try {
    const text = decodeNotebookContent((await client.readFile(cwd, LEGACY_NOTEBOOK_PATH)).bytes);
    return text.trim().length > 0 ? text : null;
  } catch {
    return null;
  }
}

function NotebookPanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target } = usePaneContext();
  const { isInteractive } = usePaneFocus();
  const {
    settings: { experimentalFeatures },
  } = useAppSettings();
  const client = useHostRuntimeClient(serverId);
  const supported = useHostFeature(serverId, "workspaceNotepad");
  const workspaceDirectory = useWorkspaceDirectory(serverId, workspaceId);
  const [content, setContent] = useState("");
  const [persistedContent, setPersistedContent] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [status, setStatus] = useState<NotebookStatus>("loading");
  // The revision the editor's text is based on; a replace must carry it.
  const revisionRef = useRef(0);
  const latestContentRef = useRef(content);
  const persistedContentRef = useRef(persistedContent);
  const loadedRef = useRef(false);
  const mountedRef = useRef(true);
  const pollInFlightRef = useRef(false);
  const readRefusedRef = useRef(false);
  const legacyImportTriedRef = useRef(false);
  const workspaceDirectoryRef = useRef(workspaceDirectory);
  const saveGenerationRef = useRef(0);
  const saveInFlightRef = useRef(false);
  const saveSkippedWhileInFlightRef = useRef(false);
  const [saveRearm, setSaveRearm] = useState(0);
  const invariantTarget = target;
  invariant(invariantTarget.kind === "notebook", "NotebookPanel requires notebook target");

  const enabled = experimentalFeatures && supported && Boolean(client);
  const dirty = loaded && content !== persistedContent;
  // `loading` also covers a reload, whose answer replaces the text.
  const canEdit =
    enabled &&
    loaded &&
    isInteractive &&
    status !== "loading" &&
    status !== "readonly" &&
    status !== "conflict";

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
    workspaceDirectoryRef.current = workspaceDirectory;
  }, [workspaceDirectory]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const showNotepad = useCallback((notepad: { revision: number; text: string }) => {
    revisionRef.current = notepad.revision;
    latestContentRef.current = notepad.text;
    persistedContentRef.current = notepad.text;
    loadedRef.current = true;
    setContent(notepad.text);
    setPersistedContent(notepad.text);
    setLoaded(true);
    setStatus("clean");
  }, []);

  /**
   * `poll` shows the host's text on first load and afterwards only when it is newer and nothing
   * typed here would be lost. `reload` always replaces the editor, discarding unsaved text.
   */
  const refresh = useCallback(
    async (mode: "poll" | "reload") => {
      if (!client) return;
      if (mode === "poll") {
        if (pollInFlightRef.current || readRefusedRef.current) return;
        pollInFlightRef.current = true;
      }
      try {
        let notepad: { revision: number; text: string } =
          await client.getWorkspaceNotepad(workspaceId);
        const cwd = workspaceDirectoryRef.current;
        if (
          !loadedRef.current &&
          !legacyImportTriedRef.current &&
          notepad.revision === 0 &&
          notepad.text === "" &&
          cwd
        ) {
          // The editor is still locked, so nothing typed here can race the import.
          legacyImportTriedRef.current = true;
          const legacy = await readLegacyNotebook(client, cwd);
          const imported =
            legacy === null
              ? null
              : await client
                  .writeWorkspaceNotepad({ workspaceId, text: legacy, expected: 0 })
                  .catch(() => null);
          if (legacy !== null && imported?.accepted) {
            notepad = { revision: imported.revision, text: legacy };
          }
        }
        if (!mountedRef.current) return;
        if (mode === "reload" || !loadedRef.current) {
          showNotepad(notepad);
          return;
        }
        const unsaved = latestContentRef.current !== persistedContentRef.current;
        if (unsaved || saveInFlightRef.current || notepad.revision <= revisionRef.current) return;
        showNotepad(notepad);
      } catch (error) {
        if (!mountedRef.current) return;
        const refused = isReadOnlyError(error);
        // A guest outside the workspace is refused every time; asking again changes nothing.
        if (refused) readRefusedRef.current = true;
        // A failed background poll keeps what is shown; the next one tries again.
        if (mode === "reload" || !loadedRef.current) setStatus(refused ? "readonly" : "error");
      } finally {
        if (mode === "poll") pollInFlightRef.current = false;
      }
    },
    [client, showNotepad, workspaceId],
  );

  // The first load does not wait for focus, so a visible pane beside the focused one fills in.
  useEffect(() => {
    if (enabled) void refresh("poll");
  }, [enabled, refresh]);

  useEffect(() => {
    if (!enabled || !isInteractive) return;
    void refresh("poll");
    const timer = setInterval(() => void refresh("poll"), POLL_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [enabled, isInteractive, refresh]);

  // Debounced autosave. It is keyed on the text itself, not only on `dirty`: `dirty` stays true
  // from the first keystroke until everything is written, so an effect keyed on it alone runs
  // once and never saves what was typed during or after that first write.
  useEffect(() => {
    if (!dirty || !canEdit || !client) return;
    const generation = saveGenerationRef.current;
    const timer = setTimeout(() => {
      // One write at a time: a second one would carry the revision the first is about to
      // replace and come back as a conflict. The write in flight re-arms this effect when it
      // settles.
      if (saveInFlightRef.current) {
        saveSkippedWhileInFlightRef.current = true;
        return;
      }
      const expected = revisionRef.current;
      const nextContent = latestContentRef.current;
      saveInFlightRef.current = true;
      setStatus("saving");
      void client
        .writeWorkspaceNotepad({ workspaceId, text: nextContent, expected })
        .then((result) => {
          if (result.accepted) {
            // Recorded even for a superseded save: the host's revision moved either way, and a
            // stale one would turn the next save into a conflict.
            revisionRef.current = result.revision;
            persistedContentRef.current = nextContent;
            setPersistedContent(nextContent);
          }
          if (generation !== saveGenerationRef.current) return;
          if (result.accepted) {
            setStatus(latestContentRef.current === nextContent ? "saved" : "unsaved");
          } else {
            // The host answers with its current revision. An unchanged one means it refused
            // the text itself, such as one over the size limit, not that someone else wrote.
            setStatus(result.revision === expected ? "error" : "conflict");
          }
        })
        .catch((error) => {
          if (generation === saveGenerationRef.current) {
            setStatus(isReadOnlyError(error) ? "readonly" : "error");
          }
        })
        .finally(() => {
          saveInFlightRef.current = false;
          if (saveSkippedWhileInFlightRef.current) {
            saveSkippedWhileInFlightRef.current = false;
            setSaveRearm((count) => count + 1);
          }
        });
    }, SAVE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [canEdit, client, content, dirty, persistedContent, saveRearm, workspaceId]);

  const handleChange = useCallback(
    (nextContent: string) => {
      if (!canEdit) return;
      // Set now, not in the effect: a poll answering before the next render must see this text.
      latestContentRef.current = nextContent;
      setContent(nextContent);
      setStatus("unsaved");
    },
    [canEdit],
  );

  const handleReload = useCallback(() => {
    setStatus("loading");
    void refresh("reload");
  }, [refresh]);

  if (!experimentalFeatures) {
    return (
      <View style={styles.center} testID="workspace-notebook-disabled">
        <Text style={styles.hint}>{t("panels.notebook.disabled")}</Text>
      </View>
    );
  }
  if (!supported) {
    return (
      <View style={styles.center} testID="workspace-notebook-unsupported">
        <Text style={styles.hint}>{t("panels.notebook.unsupported")}</Text>
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
          <View style={styles.statusRow}>
            <Text style={styles.status}>{statusLabel}</Text>
            {status === "conflict" ? (
              <Pressable
                onPress={handleReload}
                accessibilityRole="button"
                testID="workspace-notebook-reload"
              >
                <Text style={styles.reload}>{t("panels.notebook.reload")}</Text>
              </Pressable>
            ) : null}
          </View>
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
