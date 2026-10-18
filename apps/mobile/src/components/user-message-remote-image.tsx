import React, { useCallback, useMemo, useState } from "react";
import { Image, Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useAssistantImage } from "@/assistant-image/use-assistant-image";
import { AttachmentFrame } from "./attachment-pill";
import { AttachmentLightbox, type ImageLightboxSource } from "./attachment-lightbox";

/** Fetch host image artifacts through the connected daemon, including on other devices. */
export function UserMessageRemoteImage({
  source,
  occurrenceKey,
  serverId,
  client,
}: {
  source: string;
  occurrenceKey: string;
  serverId?: string;
  client?: DaemonClient | null;
}) {
  const { t } = useTranslation();
  const image = useAssistantImage({ source, occurrenceKey, serverId, client });
  const [viewerOpen, setViewerOpen] = useState(false);
  const openViewer = useCallback(() => setViewerOpen(true), []);
  const closeViewer = useCallback(() => setViewerOpen(false), []);
  const binding = image.status === "failed" ? null : image.binding;
  const uri = binding?.uri;
  const preview = useMemo(() => ({ uri: uri ?? "" }), [uri]);
  const lightboxSource = useMemo<ImageLightboxSource | null>(
    () => (viewerOpen && uri ? { type: "uri", uri } : null),
    [viewerOpen, uri],
  );
  return (
    <>
      <AttachmentFrame
        onPress={image.status === "loaded" ? openViewer : undefined}
        accessibilityLabel={t("composer.attachments.openImage")}
      >
        {binding ? (
          <Image
            ref={binding.onRef}
            source={preview}
            style={styles.thumbnail}
            onLoad={binding.onLoad}
            onError={binding.onError}
          />
        ) : (
          <View
            style={styles.thumbnail}
            accessibilityLabel={
              image.status === "failed" ? image.message : t("composer.attachments.openImage")
            }
          >
            {image.status === "failed" ? <Text style={styles.error}>!</Text> : null}
          </View>
        )}
      </AttachmentFrame>
      <AttachmentLightbox source={lightboxSource} onClose={closeViewer} />
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  thumbnail: { width: 48, height: 48, backgroundColor: theme.colors.surface1 },
  error: { color: theme.colors.foregroundMuted, textAlign: "center" },
}));
