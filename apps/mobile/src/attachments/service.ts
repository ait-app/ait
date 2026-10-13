import { collectRetainedAttachmentIds } from "@/attachments/gc-retention";
import { getAttachmentStore } from "@/attachments/store";
import type { AttachmentMetadata, SaveAttachmentInput } from "@/attachments/types";
import {
  imagePartsError,
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

  const store = await getAttachmentStore();
  if (attachments.length > MAX_SEND_IMAGES) throw imagePartsError();
  const originals = new Map<number, EncodedSendImage>();
  const groups: EncodedSendImage[][] = attachments.map(() => []);
  let unreadOriginals = attachments.length;
  const prepare = async (index: number, byteLimit = MAX_ORIGINAL_IMAGE_BASE64) => {
    const maxParts =
      MAX_SEND_IMAGES -
      unreadOriginals -
      groups.reduce((count, group) => count + group.length, 0) +
      groups[index].length;
    const parts = await prepareImageForSend(originals.get(index)!, maxParts, undefined, byteLimit);
    originals.delete(index);
    return parts.map((part) => ({
      ...part,
      ...(part.description
        ? { description: `original image ${index + 1}: ${part.description}` }
        : {}),
    }));
  };
  const fitRequest = async () => {
    // A prefix over the request limit also makes the full set oversized. Reduce
    // it now so many large originals do not all remain resident in memory.
    const candidates = [...originals.entries()].sort(
      ([, a], [, b]) => b.data.length - a.data.length,
    );
    for (const [index, original] of candidates) {
      if (imagePromptBytes(groups.flat(), context) <= MAX_IMAGE_PROMPT_BYTES) break;
      if (isAnimatedSendImage(original)) continue;
      const parts = await prepare(
        index,
        Math.min(MAX_ORIGINAL_IMAGE_BASE64, original.data.length - 1),
      );
      if (parts.reduce((bytes, part) => bytes + part.data.length, 0) < original.data.length) {
        groups[index] = parts;
      }
    }
    if (imagePromptBytes(groups.flat(), context) > MAX_IMAGE_PROMPT_BYTES) {
      throw new Error("These images are too large to send together. Send fewer images.");
    }
  };
  // Read sequentially and decode only one bitmap at a time. Byte limits alone
  // trigger reduction; normalization of unsupported formats is separate.
  for (const [index, attachment] of attachments.entries()) {
    const data = await store.encodeBase64({ attachment });
    const original = { data, mimeType: attachment.mimeType };
    unreadOriginals -= 1;
    originals.set(index, original);
    groups[index] = [original];
    if (data.length > MAX_ORIGINAL_IMAGE_BASE64 || !supportsSendImage(original.mimeType)) {
      groups[index] = await prepare(index);
    }
    if (imagePromptBytes(groups.flat(), context) > MAX_IMAGE_PROMPT_BYTES) await fitRequest();
  }
  const encoded = groups.flat();
  return encoded;
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
