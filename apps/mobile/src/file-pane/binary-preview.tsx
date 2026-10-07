import { useCallback, useState } from "react";
import { Platform, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import * as Sharing from "expo-sharing";
import type { AttachmentMetadata } from "@/attachments/types";
import { Button } from "@/components/ui/button";
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
  const open = useCallback(async () => {
    if (!attachment || !uri) return;
    setOpening(true);
    setError(null);
    try {
      const opener = getDesktopHost()?.opener?.openFile;
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
  }, [attachment, uri]);
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 12 }}>
      {pdf && uri ? (
        <FilePdfPreview uri={uri} />
      ) : (
        <Text>{t("panels.file.binaryPreviewUnavailable")}</Text>
      )}
      <Button disabled={!uri || !attachment || opening} onPress={() => void open()}>
        <Text>{t("panels.file.openWithSystem")}</Text>
      </Button>
      {error ? <Text accessibilityRole="alert">{error}</Text> : null}
    </View>
  );
}
