import { collectRetainedAttachmentIds } from "@/attachments/gc-retention";
import { getAttachmentStore } from "@/attachments/store";
import type { AttachmentMetadata, SaveAttachmentInput } from "@/attachments/types";
import {
  imagePromptBytes,
  isAnimatedSendImage,
  MAX_IMAGE_PROMPT_BYTES,
  MAX_ORIGINAL_IMAGE_BASE64,
  MAX_SEND_IMAGES,
  prepareImageForSend,
  supportsSendImage,
  type EncodedSendImage,
  type ImageSendContext,
} from "./image-send";

const activePersistence = new Set<Promise<AttachmentMetadata>>();
const persistedDuringGarbageCollection = new Set<string>();
let pendingGarbageCollections = 0;
let garbageCollectionTail: Promise<void> = Promise.resolve();
let persistenceBarrier: Promise<void> | null = null;
let releasePersistenceBarrier: (() => void) | null = null;

async function waitForPersistenceBarrier(): Promise<void> {
  const barrier = persistenceBarrier;
  if (!barrier) {
    return;
  }
  await barrier;
  await waitForPersistenceBarrier();
}

async function persistAttachment(input: SaveAttachmentInput): Promise<AttachmentMetadata> {
  await waitForPersistenceBarrier();
  const pending = (async () => {
    const store = await getAttachmentStore();
    const attachment = await store.save(input);
    if (pendingGarbageCollections > 0) {
      persistedDuringGarbageCollection.add(attachment.id);
    }
    return attachment;
  })();
  activePersistence.add(pending);
  try {
    return await pending;
  } finally {
    activePersistence.delete(pending);
  }
}

export async function persistAttachmentFromBlob(input: {
  blob: Blob;
  mimeType?: string;
  fileName?: string | null;
  id?: string;
}): Promise<AttachmentMetadata> {
  return await persistAttachment({
    id: input.id,
    mimeType: input.mimeType,
    fileName: input.fileName,
    source: { kind: "blob", blob: input.blob },
  });
}

export async function persistAttachmentFromDataUrl(input: {
  dataUrl: string;
  mimeType?: string;
  fileName?: string | null;
  id?: string;
}): Promise<AttachmentMetadata> {
  return await persistAttachment({
    id: input.id,
    mimeType: input.mimeType,
    fileName: input.fileName,
    source: { kind: "data_url", dataUrl: input.dataUrl },
  });
}

export async function persistAttachmentFromBytes(input: {
  bytes: Uint8Array;
  mimeType?: string;
  fileName?: string | null;
  id?: string;
}): Promise<AttachmentMetadata> {
  return await persistAttachment({
    id: input.id,
    mimeType: input.mimeType,
    fileName: input.fileName,
    source: { kind: "bytes", bytes: input.bytes },
  });
}

export async function persistAttachmentFromFileUri(input: {
  uri: string;
  mimeType?: string;
  fileName?: string | null;
  id?: string;
}): Promise<AttachmentMetadata> {
  return await persistAttachment({
    id: input.id,
    mimeType: input.mimeType,
    fileName: input.fileName,
    source: { kind: "file_uri", uri: input.uri },
  });
}

export async function encodeAttachmentsForSend(
  attachments: readonly AttachmentMetadata[] | undefined,
  context: ImageSendContext = {},
): Promise<EncodedSendImage[] | undefined> {
  if (!attachments || attachments.length === 0) {
    return undefined;
  }

  if (attachments.length > MAX_SEND_IMAGES) {
    throw new Error(`A message can contain at most ${MAX_SEND_IMAGES} images. Send fewer images.`);
  }
  if (imagePromptBytes([], context) >= MAX_IMAGE_PROMPT_BYTES) {
    throw new Error("The message context is too large. Shorten the text or remove attachments.");
  }
  const store = await getAttachmentStore();
  const images: EncodedSendImage[] = [];
  const animated = new Set<number>();
  const fitRequest = async (freshIndex: number, freshOriginal: EncodedSendImage) => {
    let excess = imagePromptBytes(images, context) - MAX_IMAGE_PROMPT_BYTES;
    while (excess > 0) {
      let largest = -1;
      let reducibleBytes = 0;
      for (const [index, image] of images.entries()) {
        if (!animated.has(index)) {
          reducibleBytes += image.data.length;
          if (largest < 0 || image.data.length > images[largest].data.length) largest = index;
        }
      }
      if (largest < 0 || reducibleBytes <= excess) {
        throw new Error("These images are too large to send together. Send fewer images.");
      }
      const current = images[largest];
      // Share a large deficit across images instead of demanding a near-zero-byte image.
      const byteLimit = Math.min(
        MAX_ORIGINAL_IMAGE_BASE64,
        Math.max(Math.floor(current.data.length / 2), current.data.length - excess),
      );
      // Re-read an earlier original only when needed, avoiding accumulated lossy re-encoding.
      const original =
        largest === freshIndex
          ? freshOriginal
          : {
              data: await store.encodeBase64({ attachment: attachments[largest] }),
              mimeType: attachments[largest].mimeType,
            };
      images[largest] = await prepareImageForSend(original, undefined, byteLimit);
      excess = imagePromptBytes(images, context) - MAX_IMAGE_PROMPT_BYTES;
    }
  };
  // Read sequentially and decode only one bitmap at a time. Byte limits alone
  // trigger reduction; normalization of unsupported formats is separate.
  for (const [index, attachment] of attachments.entries()) {
    const data = await store.encodeBase64({ attachment });
    const original = { data, mimeType: attachment.mimeType };
    if (isAnimatedSendImage(original)) animated.add(index);
    images.push(
      data.length > MAX_ORIGINAL_IMAGE_BASE64 || !supportsSendImage(original.mimeType)
        ? await prepareImageForSend(original)
        : original,
    );
    if (imagePromptBytes(images, context) > MAX_IMAGE_PROMPT_BYTES)
      await fitRequest(index, original);
  }
  return images;
}

export async function resolveAttachmentPreviewUrl(attachment: AttachmentMetadata): Promise<string> {
  const store = await getAttachmentStore();
  return await store.resolvePreviewUrl({ attachment });
}

export async function releaseAttachmentPreviewUrl(input: {
  attachment: AttachmentMetadata;
  url: string;
}): Promise<void> {
  const store = await getAttachmentStore();
  if (!store.releasePreviewUrl) {
    return;
  }
  await store.releasePreviewUrl({ attachment: input.attachment, url: input.url });
}

export async function deleteAttachments(
  attachments: readonly AttachmentMetadata[] | undefined,
): Promise<void> {
  if (!attachments || attachments.length === 0) {
    return;
  }
  const store = await getAttachmentStore();
  await Promise.all(
    attachments.map(async (attachment) => {
      try {
        await store.delete({ attachment });
      } catch (error) {
        console.warn("[attachments] Failed to delete attachment", {
          id: attachment.id,
          error,
        });
      }
    }),
  );
}

export async function garbageCollectAttachments(input: {
  referencedIds: ReadonlySet<string>;
}): Promise<void> {
  pendingGarbageCollections += 1;
  if (!persistenceBarrier) {
    persistenceBarrier = new Promise<void>((resolve) => {
      releasePersistenceBarrier = resolve;
    });
  }

  const previousGarbageCollection = garbageCollectionTail;
  const currentGarbageCollection = (async () => {
    await previousGarbageCollection;
    while (activePersistence.size > 0) {
      await Promise.allSettled(activePersistence);
    }
    const referencedIds = new Set(input.referencedIds);
    for (const id of collectRetainedAttachmentIds()) {
      referencedIds.add(id);
    }
    for (const id of persistedDuringGarbageCollection) {
      referencedIds.add(id);
    }
    const store = await getAttachmentStore();
    await store.garbageCollect({ referencedIds });
  })();
  garbageCollectionTail = currentGarbageCollection.catch(() => undefined);

  try {
    await currentGarbageCollection;
  } finally {
    pendingGarbageCollections -= 1;
    if (pendingGarbageCollections === 0) {
      persistedDuringGarbageCollection.clear();
      const release = releasePersistenceBarrier;
      releasePersistenceBarrier = null;
      persistenceBarrier = null;
      release?.();
    }
  }
}
