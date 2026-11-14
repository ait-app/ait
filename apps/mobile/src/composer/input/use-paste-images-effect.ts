import { useEffect, useRef } from "react";
import type { ImageAttachment } from "@/composer/types";
import { isWeb } from "@/constants/platform";
import {
  collectImageFilesFromClipboardData,
  filesToImageAttachments,
} from "@/attachments/image-attachments-from-files";

interface PasteImagesEffectArgs {
  textarea: HTMLElement | null;
  isConnected: boolean;
  disabled: boolean;
  isDictating: boolean;
  isRealtimeVoiceForCurrentAgent: boolean;
  onAddImages: ((images: ImageAttachment[]) => void) | undefined;
  trackImagePasteSave?: (save: Promise<unknown>) => void;
}

export function usePasteImagesEffect(args: PasteImagesEffectArgs): void {
  const {
    textarea,
    isConnected,
    disabled,
    isDictating,
    isRealtimeVoiceForCurrentAgent,
    onAddImages,
    trackImagePasteSave,
  } = args;

  // Read after the save so a re-subscribe (disabled/isConnected changing) cannot drop the image;
  // only an unmount clears it.
  const onAddImagesRef = useRef(onAddImages);
  useEffect(() => {
    onAddImagesRef.current = onAddImages;
    return () => {
      onAddImagesRef.current = undefined;
    };
  }, [onAddImages]);

  useEffect(() => {
    if (!isWeb || !onAddImages) return;

    if (!textarea) return;

    const handlePaste = (event: ClipboardEvent) => {
      if (!isConnected || disabled || isDictating || isRealtimeVoiceForCurrentAgent) return;

      const imageFiles = collectImageFilesFromClipboardData(event.clipboardData);
      if (imageFiles.length === 0) return;

      event.preventDefault();

      const save = filesToImageAttachments(imageFiles)
        .then((pastedAttachments) => {
          if (pastedAttachments.length === 0) return;
          onAddImagesRef.current?.(pastedAttachments);
          return;
        })
        .catch((error) => {
          console.error("[MessageInput] Failed to process pasted images:", error);
        });
      trackImagePasteSave?.(save);
    };

    textarea.addEventListener("paste", handlePaste);
    return () => {
      textarea.removeEventListener?.("paste", handlePaste);
    };
  }, [
    disabled,
    textarea,
    isConnected,
    isDictating,
    isRealtimeVoiceForCurrentAgent,
    onAddImages,
    trackImagePasteSave,
  ]);
}
