import { useCallback, useState } from "react";
import { Platform, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Download, ExternalLink, Share2 } from "lucide-react-native";
import * as Sharing from "expo-sharing";
import type { AttachmentMetadata } from "@/attachments/types";
import { Button } from "@/components/ui/button";
import { MaterialFileIcon } from "@/components/material-file-icon";
import { getDesktopHost } from "@/desktop/host";
import { fileUriToPath } from "@/attachments/utils";
import { canPreviewPdf, FilePdfPreview } from "./pdf-preview";

export function FileBinaryPreview({
  attachment,
  uri,
}: {
  attachment: AttachmentMetadata | null;
  uri: string | null;
}) {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const pdf = attachment?.mimeType === "application/pdf" && canPreviewPdf();
  const opener = getDesktopHost()?.opener?.openFile;
  const opensDesktopFile = attachment?.storageType === "desktop-file" && Boolean(opener);
  const downloads = Platform.OS === "web" && !opensDesktopFile;
  const actionLabel = opensDesktopFile
    ? t("panels.file.openWithSystem")
    : downloads
      ? t("panels.file.downloadFile")
      : t("panels.file.shareFile");
  const open = useCallback(async () => {
    if (!attachment || !uri) return;
    setOpening(true);
    setError(null);
    try {
      if (attachment.storageType === "desktop-file" && opener) {
        await opener(fileUriToPath(attachment.storageKey));
      } else if (Platform.OS !== "web") {
        await Sharing.shareAsync(uri, { mimeType: attachment.mimeType });
      } else {
        const link = document.createElement("a");
        link.href = uri;
        link.download = attachment.fileName || "download";
        link.click();
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setOpening(false);
    }
  }, [attachment, uri, opener]);
  const action = (
    <Button
      variant="outline"
      size="sm"
      leftIcon={opensDesktopFile ? ExternalLink : downloads ? Download : Share2}
      style={styles.action}
      textStyle={styles.actionText}
      disabled={!uri || !attachment}
      loading={opening}
      onPress={() => void open()}
    >
      {actionLabel}
    </Button>
  );
  const failure = error ? (
    <Text style={styles.error} accessibilityRole="alert">
      {error}
    </Text>
  ) : null;

  if (pdf && uri) {
    return (
      <View style={styles.container}>
        <FilePdfPreview uri={uri} />
        <View style={styles.pdfActions}>
          {action}
          {failure}
        </View>
      </View>
    );
  }

  return (
    <ScrollView style={styles.container} contentContainerStyle={styles.emptyState}>
      <View style={styles.message} testID="file-binary-preview">
        <View style={styles.icon}>
          <MaterialFileIcon fileName={attachment?.fileName ?? ""} size={48} />
        </View>
        {attachment?.fileName ? <Text style={styles.filename}>{attachment.fileName}</Text> : null}
        <Text style={styles.title}>{t("panels.file.binaryPreviewUnavailable")}</Text>
        <Text style={styles.description}>{t("panels.file.binaryPreviewDescription")}</Text>
        <View style={styles.actions}>
          {action}
          {failure}
        </View>
      </View>
    </ScrollView>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { flex: 1, minHeight: 0 },
  emptyState: {
    flexGrow: 1,
    alignItems: "center",
    justifyContent: "center",
    padding: theme.spacing[6],
  },
  message: { width: "100%", maxWidth: 420, alignItems: "center", gap: theme.spacing[3] },
  icon: { marginBottom: theme.spacing[1] },
  filename: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    textAlign: "center",
  },
  title: { color: theme.colors.foreground, fontSize: theme.fontSize.base, textAlign: "center" },
  description: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.sm * 1.5,
    textAlign: "center",
  },
  actions: {
    width: "100%",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingTop: theme.spacing[2],
  },
  action: { alignSelf: "center", maxWidth: "100%", paddingVertical: theme.spacing[2] },
  actionText: { flexShrink: 1, textAlign: "center" },
  error: { color: theme.colors.statusDanger, fontSize: theme.fontSize.sm, textAlign: "center" },
  pdfActions: {
    flexShrink: 0,
    alignItems: "center",
    gap: theme.spacing[2],
    padding: theme.spacing[3],
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
  },
}));
