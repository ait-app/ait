import { afterEach, describe, expect, it, vi } from "vitest";
import type { AttachmentMetadata, AttachmentStore, SaveAttachmentInput } from "@/attachments/types";
import { __setAttachmentStoreForTests } from "./store";
import {
  encodeAttachmentsForSend,
  garbageCollectAttachments,
  persistAttachmentFromBytes,
} from "./service";

vi.mock("./image-send-codec", () => ({
  openSendImage: async (uri: string) => ({
    width: 800,
    height: uri.includes("att_long") ? 6000 : 500,
    encode: async () => ({ data: "part", mimeType: "image/png" }),
    release() {},
  }),
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
    const original = createAttachment({ id: "att_long", byteSize: 4000000 });
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
    const attachments = Array.from({ length: 50 }, (_, index) =>
      createAttachment({ id: index === 0 ? "att_long" : `att_${index}` }),
    );
    await expect(encodeAttachmentsForSend(attachments)).rejects.toThrow("too many readable parts");
  });

  it("rejects an oversized aggregate instead of sending a partial set", async () => {
    const store = createRecordingStore();
    __setAttachmentStoreForTests(store);
    vi.spyOn(store, "encodeBase64").mockResolvedValue("A".repeat(16 * 1024 * 1024));
    const originals = Array.from({ length: 4 }, (_, index) =>
      createAttachment({ id: `animated_${index}`, mimeType: "image/gif" }),
    );
    await expect(encodeAttachmentsForSend(originals)).rejects.toThrow("too large to send together");
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
