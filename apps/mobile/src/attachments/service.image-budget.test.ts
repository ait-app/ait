import { afterEach, expect, it, vi } from "vitest";
import type { AttachmentMetadata, AttachmentStore } from "./types";
import { __setAttachmentStoreForTests } from "./store";
import { encodeAttachmentsForSend } from "./service";
import { imagePromptBytes, MAX_IMAGE_PROMPT_BYTES } from "./image-send";
import { openSendImage } from "./image-send-codec";

vi.mock("./image-send", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./image-send")>()),
  MAX_IMAGE_PROMPT_BYTES: 1024,
}));
vi.mock("./image-send-codec", () => ({
  openSendImage: vi.fn(async () => ({
    width: 1000,
    height: 1000,
    encode: async (size: { width: number }, mimeType: string) => ({
      data: "P".repeat(Math.floor(750 * (size.width / 1000) ** 2)),
      mimeType,
    }),
    release() {},
  })),
}));

afterEach(() => {
  __setAttachmentStoreForTests(null);
  vi.clearAllMocks();
});

function fixture() {
  const attachments: AttachmentMetadata[] = ["image/png", "image/gif", "image/gif"].map(
    (mimeType, index) => ({
      id: `${index}`,
      mimeType,
      storageType: "web-indexeddb",
      storageKey: `${index}`,
      createdAt: 1,
    }),
  );
  const original = "A".repeat(800);
  const read = vi.fn(async ({ attachment }: { attachment: AttachmentMetadata }) =>
    attachment.id === "0" ? original : "G".repeat(240),
  );
  const store: AttachmentStore = {
    storageType: "web-indexeddb",
    encodeBase64: read,
    save: vi.fn(),
    resolvePreviewUrl: vi.fn(),
    delete: vi.fn(),
    garbageCollect: vi.fn(),
  };
  __setAttachmentStoreForTests(store);
  return { attachments, original, read };
}

it("reprocesses an earlier original when later images consume the remaining request budget", async () => {
  const { attachments, original, read } = fixture();
  const result = await encodeAttachmentsForSend(attachments);
  expect(result).toHaveLength(3);
  expect(result?.slice(1)).toEqual([
    { data: "G".repeat(240), mimeType: "image/gif" },
    { data: "G".repeat(240), mimeType: "image/gif" },
  ]);
  expect(imagePromptBytes(result!, {})).toBeLessThanOrEqual(MAX_IMAGE_PROMPT_BYTES);
  expect(openSendImage).toHaveBeenCalledTimes(2);
  expect(
    vi
      .mocked(openSendImage)
      .mock.calls.every(([uri]) => uri === `data:image/png;base64,${original}`),
  ).toBe(true);
  expect(read.mock.calls.filter(([input]) => input.attachment.id === "0").length).toBeGreaterThan(
    1,
  );
});

it("rejects context that cannot fit before reading image files", async () => {
  const { attachments, read } = fixture();
  await expect(encodeAttachmentsForSend(attachments, { text: "x".repeat(1024) })).rejects.toThrow(
    "message context is too large",
  );
  expect(read).not.toHaveBeenCalled();
});

it("rejects an aggregate that cannot fit even if every static image were removed", async () => {
  const { attachments, read } = fixture();
  read.mockImplementation(async ({ attachment }) =>
    attachment.id === "0" ? "A".repeat(100) : "G".repeat(500),
  );
  await expect(encodeAttachmentsForSend(attachments)).rejects.toThrow("too large to send together");
  expect(openSendImage).not.toHaveBeenCalled();
});
