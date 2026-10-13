import { afterEach, describe, expect, it, vi } from "vitest";
import type { AttachmentMetadata, AttachmentStore, SaveAttachmentInput } from "@/attachments/types";
import { MAX_ORIGINAL_IMAGE_BASE64, MAX_IMAGE_PROMPT_BYTES, imagePromptBytes } from "./image-send";
import { openSendImage } from "./image-send-codec";
import { __setAttachmentStoreForTests } from "./store";
import {
  encodeAttachmentsForSend,
  garbageCollectAttachments,
  persistAttachmentFromBytes,
} from "./service";

vi.mock("./image-send-codec", () => ({
  openSendImage: vi.fn(async (uri: string) => ({
    width: 800,
    height: uri.includes("att_long") ? 6000 : 500,
    encode: async () => ({ data: "part", mimeType: "image/png" }),
    release() {},
  })),
}));

function createAttachment(input: Partial<AttachmentMetadata> = {}): AttachmentMetadata {
  return {
    id: input.id ?? "att_1",
    mimeType: input.mimeType ?? "image/png",
    storageType: input.storageType ?? "web-indexeddb",
    storageKey: input.storageKey ?? "att_1",
    fileName: input.fileName,
    byteSize: input.byteSize,
    createdAt: input.createdAt ?? 1700000000000,
  };
}

function createRecordingStore(): AttachmentStore & {
  savedSources: SaveAttachmentInput[];
  releasedUrls: string[];
} {
  const savedSources: SaveAttachmentInput[] = [];
  const releasedUrls: string[] = [];

  return {
    storageType: "web-indexeddb",
    savedSources,
    releasedUrls,
    async save(input) {
      savedSources.push(input);
      return createAttachment({
        id: input.id,
        mimeType: input.mimeType,
        fileName: input.fileName,
        byteSize: 4,
      });
    },
    async encodeBase64({ attachment }) {
      return `${attachment.id}:base64`;
    },
    async resolvePreviewUrl({ attachment }) {
      return `blob:${attachment.id}`;
    },
    async releasePreviewUrl({ url }) {
      releasedUrls.push(url);
    },
    async delete() {},
    async garbageCollect() {},
  };
}

describe("attachment service", () => {
  afterEach(() => {
    __setAttachmentStoreForTests(null);
    vi.clearAllMocks();
  });

  it("persists raw bytes without requiring a base64 wrapper", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    const bytes = new Uint8Array([0, 1, 2, 3]);

    const attachment = await persistAttachmentFromBytes({
      id: "att_bytes",
      bytes,
      mimeType: "image/png",
      fileName: "image.png",
    });

    expect(attachment).toEqual({
      id: "att_bytes",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "att_1",
      fileName: "image.png",
      byteSize: 4,
      createdAt: 1700000000000,
    });
    expect(store.savedSources).toEqual([
      {
        id: "att_bytes",
        mimeType: "image/png",
        fileName: "image.png",
        source: { kind: "bytes", bytes },
      },
    ]);
  });

  it("keeps provider send output byte-compatible", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    const attachment = createAttachment({ id: "att_send", mimeType: "image/jpeg" });

    await expect(encodeAttachmentsForSend([attachment])).resolves.toEqual([
      { data: "att_send:base64", mimeType: "image/jpeg" },
    ]);
  });

  it("expands a long screenshot in order while preserving original storage metadata", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    vi.spyOn(store, "encodeBase64").mockResolvedValue(
      "att_long" + "A".repeat(MAX_ORIGINAL_IMAGE_BASE64),
    );
    const original = createAttachment({ id: "att_long", byteSize: 30000000 });
    const before = { ...original };
    const result = await encodeAttachmentsForSend([original]);
    expect(result?.length).toBeGreaterThan(1);
    expect(result?.[0].description).toContain("original image 1: overview");
    expect(result?.[1].description).toContain("original image 1: detail crop");
    expect(original).toEqual(before);
    expect(store.savedSources).toEqual([]);
  });

  it("fails the whole send if an attachment cannot be read instead of silently omitting it", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    vi.spyOn(store, "encodeBase64").mockImplementation(async ({ attachment }) => {
      if (attachment.id === "missing") throw new Error("missing attachment");
      return "base64";
    });
    await expect(
      encodeAttachmentsForSend([createAttachment(), createAttachment({ id: "missing" })]),
    ).rejects.toThrow("missing attachment");
  });

  it("reserves space for every selected original when expanding images", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    const read = store.encodeBase64.bind(store);
    vi.spyOn(store, "encodeBase64").mockImplementation(async (input) =>
      input.attachment.id === "att_long"
        ? "att_long" + "A".repeat(MAX_ORIGINAL_IMAGE_BASE64)
        : read(input),
    );
    const attachments = Array.from({ length: 50 }, (_, index) =>
      createAttachment({ id: index === 0 ? "att_long" : `att_${index}` }),
    );
    await expect(encodeAttachmentsForSend(attachments)).rejects.toThrow("too many readable parts");
  });

  it("keeps all fifty under-budget images without geometry processing", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    const originals = Array.from({ length: 50 }, (_, index) =>
      createAttachment({ id: `att_long_${index}`, byteSize: 4000000 }),
    );
    const result = await encodeAttachmentsForSend(originals);
    expect(result).toHaveLength(50);
    expect(result?.[0].data).toBe("att_long_0:base64");
    expect(openSendImage).not.toHaveBeenCalled();
  });

  it("compresses only enough originals to fit an oversized aggregate and preserves order", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    vi.spyOn(store, "encodeBase64").mockImplementation(
      async ({ attachment }) => attachment.id + "A".repeat(16 * 1024 * 1024),
    );
    const originals = Array.from({ length: 4 }, (_, index) =>
      createAttachment({ id: `image${index}` }),
    );
    const result = await encodeAttachmentsForSend(originals);
    expect(result).toHaveLength(4);
    expect(result?.[0].data).toBe("part");
    expect(result?.[1].data.startsWith("image1")).toBe(true);
    expect(result?.[3].data.startsWith("image3")).toBe(true);
    expect(openSendImage).toHaveBeenCalledOnce();
    expect(imagePromptBytes(result!, {})).toBeLessThanOrEqual(MAX_IMAGE_PROMPT_BYTES);
  });

  it("uses serialized request bytes including text rather than a lower image-only threshold", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    const images = [
      { data: "", mimeType: "image/png" },
      { data: "", mimeType: "image/png" },
    ];
    const overhead = imagePromptBytes(images, { text: "中文" });
    images[0].data = "A".repeat(MAX_ORIGINAL_IMAGE_BASE64);
    images[1].data = "A".repeat(
      Math.floor((MAX_IMAGE_PROMPT_BYTES - overhead - MAX_ORIGINAL_IMAGE_BASE64) / 4) * 4,
    );
    vi.spyOn(store, "encodeBase64").mockImplementation(
      async ({ attachment }) => images[Number(attachment.id)].data,
    );
    const originals = [createAttachment({ id: "0" }), createAttachment({ id: "1" })];
    const gap = MAX_IMAGE_PROMPT_BYTES - imagePromptBytes(images, { text: "中文" });
    const context = { text: "中文" + "x".repeat(gap) };
    expect(imagePromptBytes(images, context)).toBe(MAX_IMAGE_PROMPT_BYTES);
    const unchanged = await encodeAttachmentsForSend(originals, context);
    expect(unchanged?.[0].data.length).toBe(MAX_ORIGINAL_IMAGE_BASE64);
    expect(openSendImage).not.toHaveBeenCalled();
    const overBudget = { text: context.text + "x" };
    const result = await encodeAttachmentsForSend(originals, overBudget);
    expect(result?.[0].data).toBe("part");
    expect(result?.[1].data.length).toBe(images[1].data.length);
    expect(openSendImage).toHaveBeenCalledOnce();
    expect(imagePromptBytes(result!, overBudget)).toBeLessThanOrEqual(MAX_IMAGE_PROMPT_BYTES);
  });

  it("rejects an oversized animated aggregate before loading the remaining files", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    vi.spyOn(store, "encodeBase64").mockResolvedValue("A".repeat(16 * 1024 * 1024));
    const originals = Array.from({ length: 5 }, (_, index) =>
      createAttachment({ id: `animated_${index}`, mimeType: "image/gif" }),
    );
    await expect(encodeAttachmentsForSend(originals)).rejects.toThrow("too large to send together");
    expect(store.encodeBase64).toHaveBeenCalledTimes(4);
    expect(store.savedSources).toEqual([]);
  });

  it("does not collect an attachment persisted while garbage collection is starting", async () => {
    let releaseSave: () => void = () => undefined;
    let reportSaveStarted: () => void = () => undefined;
    const saveStarted = new Promise<void>((resolve) => {
      reportSaveStarted = resolve;
    });
    const saveGate = new Promise<void>((resolve) => {
      releaseSave = resolve;
    });
    const garbageCollections: string[][] = [];
    const store: AttachmentStore = {
      ...createRecordingStore(),
      async save(input) {
        reportSaveStarted();
        await saveGate;
        return createAttachment({ id: input.id });
      },
      async garbageCollect({ referencedIds }) {
        garbageCollections.push([...referencedIds]);
      },
    };
    __setAttachmentStoreForTests(store);

    const persist = persistAttachmentFromBytes({
      id: "assistant-preview",
      bytes: new Uint8Array([1, 2, 3]),
      mimeType: "image/png",
    });
    await saveStarted;
    const collect = garbageCollectAttachments({ referencedIds: new Set() });

    try {
      await Promise.resolve();
      expect(garbageCollections).toEqual([]);
    } finally {
      releaseSave();
      await Promise.all([persist, collect]);
    }

    expect(garbageCollections).toEqual([["assistant-preview"]]);
  });
});
