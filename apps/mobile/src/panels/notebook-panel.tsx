import { useEffect, useMemo, useState } from "react";
import { Text, View } from "react-native";
import invariant from "tiny-invariant";
import { useTranslation } from "react-i18next";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { mutedIconColorMapping } from "@/components/ui/icon-color";
import { LoadingSpinner } from "@/components/ui/loading-spinner";
import { isWeb } from "@/constants/platform";
import { FilePane } from "@/file-pane/pane";
import { useAppSettings } from "@/hooks/use-settings";
import { useHostFeature } from "@/runtime/host-features";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { usePaneContext } from "@/panels/pane-context";
import { definePanel } from "@/panels/panel-registry";
import { notebookPanelPresentation } from "@/panels/panel-presentations";

const ThemedLoadingSpinner = withUnistyles(LoadingSpinner);

const styles = StyleSheet.create((theme) => ({
  center: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    gap: theme.spacing[3],
    padding: theme.spacing[6],
    backgroundColor: theme.colors.surface0,
  },
  hint: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    lineHeight: 21,
    textAlign: "center",
  },
  error: {
    color: theme.colors.destructive,
    fontSize: theme.fontSize.base,
    textAlign: "center",
  },
}));

/** Where the host keeps a workspace's notepad file. */
interface NotepadFile {
  workspaceId: string;
  directory: string;
  name: string;
}

type NotepadLookup =
  | { status: "loading" }
  | { status: "ready"; file: NotepadFile }
  | { status: "error"; workspaceId: string };

/**
 * The workspace notepad is a Markdown file the host keeps outside the checkout. The host says
 * where it is, creating it first if needed, and the file pane opens it like any other file.
 */
function NotebookPanel() {
  const { t } = useTranslation();
  const { serverId, workspaceId, target } = usePaneContext();
  const {
    settings: { experimentalFeatures },
  } = useAppSettings();
  const client = useHostRuntimeClient(serverId);
  const supported = useHostFeature(serverId, "workspaceNotepad");
  const [lookup, setLookup] = useState<NotepadLookup>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);
  invariant(target.kind === "notebook", "NotebookPanel requires notebook target");

  const enabled = experimentalFeatures && supported;
  // A file found for this workspace stays open while the host is away; the file pane reports
  // the disconnect and reads again when the host is back.
  const file =
    lookup.status === "ready" && lookup.file.workspaceId === workspaceId ? lookup.file : null;
  const failed = lookup.status === "error" && lookup.workspaceId === workspaceId;
  const located = file !== null;

  useEffect(() => {
    if (!enabled || !client || located) return;
    let cancelled = false;
    setLookup({ status: "loading" });
    client.getWorkspaceNotepad(workspaceId).then(
      (notepad) => {
        if (cancelled) return;
        // The file is kept directly in `directory`, which is the file methods' `cwd`.
        const name = notepad.path.split(/[\\/]/).findLast(Boolean) ?? notepad.path;
        setLookup({
          status: "ready",
          file: { workspaceId, directory: notepad.directory, name },
        });
      },
      () => {
        if (!cancelled) setLookup({ status: "error", workspaceId });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [attempt, client, enabled, located, workspaceId]);

  const location = useMemo(() => (file ? { path: file.name } : null), [file]);

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
  if (file && location) {
    return (
      <FilePane
        serverId={serverId}
        workspaceRoot={file.directory}
        location={location}
        navigationRevision={0}
        // Notes are typed more often than read; only the web file pane has an editor.
        initialMode={isWeb ? "source" : "preview"}
      />
    );
  }
  if (failed) {
    return (
      <View style={styles.center} testID="workspace-notebook-error">
        <Text style={styles.error}>{t("panels.notebook.loadFailed")}</Text>
        <Button variant="outline" size="sm" onPress={() => setAttempt((count) => count + 1)}>
          {t("common.actions.retry")}
        </Button>
      </View>
    );
  }
  return (
    <View style={styles.center} testID="workspace-notebook-loading">
      <ThemedLoadingSpinner size="small" uniProps={mutedIconColorMapping} />
      <Text style={styles.hint}>{t("panels.notebook.loading")}</Text>
    </View>
  );
}

export const notebookPanelRegistration = definePanel("notebook", {
  component: NotebookPanel,
  presentation: notebookPanelPresentation,
});
