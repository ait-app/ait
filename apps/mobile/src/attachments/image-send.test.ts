import { describe, expect, it, vi } from "vitest";
import { Buffer } from "buffer";
import {
  imagePromptBytes,
  isAnimatedSendImage,
  MAX_ORIGINAL_IMAGE_BASE64,
  prepareImageForSend,
  type SendImageSource,
} from "./image-send";

function source(width: number, height: number): SendImageSource {
  return {
    width,
    height,
    encode: vi.fn(async () => ({ data: "encoded", mimeType: "image/png" })),
    release: vi.fn(),
  };
}

describe("whole image send preparation", () => {
  it("sends one whole image after exceeding its byte budget without model-facing layout notes", async () => {
    const image = source(4284, 5712);
    image.encode = vi.fn(async (_size, mimeType) => ({ data: "ok", mimeType }));
    const result = await prepareImageForSend(
      { data: "oversized", mimeType: "image/png" },
      async () => image,
      4,
    );
    expect(result).toEqual({ data: "ok", mimeType: "image/png" });
    expect(image.encode).toHaveBeenCalledOnce();
    const size = vi.mocked(image.encode).mock.calls[0][0];
    expect(Object.keys(size).sort()).toEqual(["height", "width"]);
    expect(Math.abs(size.width - (size.height * 4284) / 5712)).toBeLessThan(1);
    expect(image.release).toHaveBeenCalledOnce();
  });

  it.each([
    [500, 200],
    [800, 1000000],
    [1000000, 800],
    [60000, 60000],
  ])("preserves under-budget %ix%i files without bitmap decoding", async (width, height) => {
    const open = vi.fn(async () => source(width, height));
    const original = { data: "original", mimeType: "image/png" };
    expect(await prepareImageForSend(original, open)).toBe(original);
    expect(open).not.toHaveBeenCalled();
  });

  it("processes only bytes above the exact per-image budget", async () => {
    const image = source(500, 200);
    const open = vi.fn(async () => image);
    const original = { data: "A".repeat(MAX_ORIGINAL_IMAGE_BASE64), mimeType: "image/png" };
    expect(await prepareImageForSend(original, open)).toBe(original);
    expect(open).not.toHaveBeenCalled();
    expect(await prepareImageForSend({ ...original, data: original.data + "AAAA" }, open)).toEqual({
      data: "encoded",
      mimeType: "image/png",
    });
    expect(open).toHaveBeenCalledOnce();
  });

  it("uses the actual image budget instead of a smaller encoding target", async () => {
    const image = source(500, 200);
    const encoded = { data: "B".repeat(8 * 1024 * 1024), mimeType: "image/png" };
    image.encode = vi.fn(async () => encoded);
    const result = await prepareImageForSend(
      { data: "A".repeat(MAX_ORIGINAL_IMAGE_BASE64 + 4), mimeType: "image/png" },
      async () => image,
    );
    expect(result).toBe(encoded);
    expect(image.encode).toHaveBeenCalledOnce();
  });

  it("counts full serialized prompt bytes including UTF-8 user context", () => {
    const images = [
      { data: "AAAA", mimeType: "image/png" },
      { data: "BBBB", mimeType: "image/webp" },
    ];
    const context = {
      text: '中文\n"',
      clientMessageId: "message",
      attachments: [{ type: "text" as const, mimeType: "text/plain" as const, text: "上下文" }],
      outputSchema: { type: "object" },
    };
    expect(imagePromptBytes(images, context)).toBe(
      Buffer.byteLength(
        JSON.stringify({
          text: context.text,
          images,
          attachments: context.attachments,
          clientMessageId: context.clientMessageId,
          outputSchema: context.outputSchema,
        }),
        "utf8",
      ),
    );
  });

  it.each([
    [500, 200],
    [800, 6000],
  ])("converts unsupported %ix%i images as a whole", async (width, height) => {
    const image = source(width, height);
    expect(
      await prepareImageForSend({ data: "original", mimeType: "image/avif" }, async () => image),
    ).toEqual({ data: "encoded", mimeType: "image/png" });
    expect(image.encode).toHaveBeenCalledExactlyOnceWith({ width, height }, "image/png", 1);
    expect(image.release).toHaveBeenCalledOnce();
  });

  it("tries lossless output before high-quality compression and preserves the actual MIME", async () => {
    const image = source(1800, 1000);
    image.encode = vi.fn(async (_size, mimeType) => ({
      data: mimeType === "image/png" ? "A".repeat(132) : "small-webp",
      mimeType,
    }));
    const result = await prepareImageForSend(
      { data: "A".repeat(132), mimeType: "image/png" },
      async () => image,
      128,
    );
    expect(result).toEqual({ data: "small-webp", mimeType: "image/webp" });
    expect(vi.mocked(image.encode).mock.calls.map(([, mime, quality]) => [mime, quality])).toEqual([
      ["image/png", 1],
      ["image/webp", 0.94],
    ]);
  });

  it("reduces the whole image proportionally when encoding alone cannot fit", async () => {
    const image = source(1800, 1000);
    image.encode = vi.fn(async (size, mimeType) => ({
      data: size.width > 1100 ? "A".repeat(256) : "whole-image",
      mimeType,
    }));
    const result = await prepareImageForSend(
      { data: "A".repeat(256), mimeType: "image/png" },
      async () => image,
      128,
    );
    expect(result.data).toBe("whole-image");
    const calls = vi.mocked(image.encode).mock.calls;
    expect(calls[0][0]).toEqual({ width: 1800, height: 1000 });
    expect(calls.at(-1)![0].width).toBeLessThanOrEqual(1100);
    expect(Math.min(...calls.map(([, , quality]) => quality))).toBe(0.82);
    for (const [size] of calls) {
      expect(Math.abs(size.width - size.height * 1.8)).toBeLessThan(2);
      expect(Object.keys(size).sort()).toEqual(["height", "width"]);
    }
  });

  it("bounds output surfaces while keeping extreme aspect ratios", async () => {
    const image = source(800, 50000);
    await prepareImageForSend({ data: "oversized", mimeType: "image/png" }, async () => image, 8);
    const size = vi.mocked(image.encode).mock.calls[0][0];
    expect(size.height).toBeLessThanOrEqual(16384);
    expect(size.width * size.height).toBeLessThanOrEqual(16 * 1024 * 1024);
    expect(Math.abs(size.width - (size.height * 800) / 50000)).toBeLessThan(1);
  });

  it("fails explicitly and releases the bitmap when no complete image fits", async () => {
    const image = source(1800, 1000);
    image.encode = vi.fn(async (_size, mimeType) => ({ data: "A".repeat(256), mimeType }));
    await expect(
      prepareImageForSend({ data: "A".repeat(256), mimeType: "image/png" }, async () => image, 128),
    ).rejects.toThrow("cannot fit the message budget");
    expect(vi.mocked(image.encode).mock.calls.length).toBeLessThanOrEqual(36);
    expect(image.release).toHaveBeenCalledOnce();
  });

  it.each(["image/png", "image/jpeg", "image/webp"])(
    "checks oversized %s headers before decode",
    async (mimeType) => {
      const header = Buffer.alloc(40);
      if (mimeType === "image/png") {
        Buffer.from("89504e470d0a1a0a", "hex").copy(header);
        header.writeUInt32BE(60000, 16);
        header.writeUInt32BE(60000, 20);
        header.write("IDAT", 12);
      } else if (mimeType === "image/jpeg") {
        header.set([0xff, 0xd8, 0xff, 0xc0]);
        header.writeUInt16BE(17, 4);
        header[6] = 8;
        header.writeUInt16BE(60000, 7);
        header.writeUInt16BE(60000, 9);
      } else {
        header.write("RIFF", 0);
        header.write("WEBP", 8);
        header.write("VP8X", 12);
        header.writeUIntLE(59999, 24, 3);
        header.writeUIntLE(59999, 27, 3);
      }
      const open = vi.fn();
      await expect(
        prepareImageForSend({ data: header.toString("base64"), mimeType }, open, 1),
      ).rejects.toThrow("too large to process safely");
      expect(open).not.toHaveBeenCalled();
    },
  );

  it.each([
    [0, 1],
    [1, -1],
    [NaN, 1],
    [60000, 60000],
  ])("validates decoded dimensions %ix%i and releases the source", async (width, height) => {
    const image = source(width, height);
    await expect(
      prepareImageForSend({ data: "oversized", mimeType: "image/png" }, async () => image, 1),
    ).rejects.toThrow();
    expect(image.encode).not.toHaveBeenCalled();
    expect(image.release).toHaveBeenCalledOnce();
  });

  it("releases the source on encoding failure", async () => {
    const image = source(800, 6000);
    image.encode = vi.fn(async () => {
      throw new Error("encoder failed");
    });
    await expect(
      prepareImageForSend({ data: "oversized", mimeType: "image/png" }, async () => image, 1),
    ).rejects.toThrow("encoder failed");
    expect(image.release).toHaveBeenCalledOnce();
  });

  it.each(["image/gif", "image/webp", "image/png"])(
    "preserves animated %s and rejects over-budget animation",
    async (mimeType) => {
      const bytes = Buffer.alloc(40);
      if (mimeType === "image/webp") {
        bytes.write("VP8X", 12);
        bytes[20] = 2;
      } else if (mimeType === "image/png") {
        Buffer.from("89504e470d0a1a0a", "hex").copy(bytes);
        bytes.write("acTL", 12);
      }
      const original = { data: bytes.toString("base64"), mimeType };
      const open = vi.fn();
      expect(isAnimatedSendImage(original)).toBe(true);
      expect(await prepareImageForSend(original, open)).toBe(original);
      await expect(prepareImageForSend(original, open, 1)).rejects.toThrow(
        "animated image is too large",
      );
      expect(open).not.toHaveBeenCalled();
    },
  );

  it.each([0, -1, NaN, Infinity])(
    "rejects invalid byte budgets %s before opening an image",
    async (budget) => {
      const open = vi.fn();
      await expect(
        prepareImageForSend({ data: "original", mimeType: "image/png" }, open, budget),
      ).rejects.toThrow("byte budget must be positive");
      expect(open).not.toHaveBeenCalled();
    },
  );
});
