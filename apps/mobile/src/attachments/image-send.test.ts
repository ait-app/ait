import { describe, expect, it, vi } from "vitest";
import { Buffer } from "buffer";
import {
  imageSendPayload,
  isAnimatedSendImage,
  MAX_IMAGE_BASE64,
  planSendImage,
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

describe("image send preparation", () => {
  it.each([
    [800, 20000],
    [20000, 800],
    [6000, 5000],
    [2049, 100],
  ])("covers every pixel of %ix%i without shrinking the detail crops", (width, height) => {
    const regions = planSendImage(width, height, 50);
    expect(regions[0].overview).toBe(true);
    expect(Math.max(regions[0].outputWidth, regions[0].outputHeight)).toBe(1024);
    const crops = regions.slice(1);
    for (const crop of crops) {
      expect(crop.outputWidth).toBe(crop.width);
      expect(crop.outputHeight).toBe(crop.height);
      expect(crop.width).toBeLessThanOrEqual(2048);
      expect(crop.height).toBeLessThanOrEqual(2048);
      expect(crop.x + crop.width).toBeLessThanOrEqual(width);
      expect(crop.y + crop.height).toBeLessThanOrEqual(height);
    }
    // Check the union on both axes, including the last pixel and every seam.
    for (const [axis, size, end] of [
      ["x", "width", width],
      ["y", "height", height],
    ] as const) {
      const intervals = [...new Map(crops.map((crop) => [crop[axis], crop[size]])).entries()].sort(
        ([a], [b]) => a - b,
      );
      let covered = 0;
      for (const [start, length] of intervals) {
        expect(start).toBeLessThanOrEqual(covered);
        if (start > 0) expect(covered - start).toBeGreaterThanOrEqual(128);
        covered = Math.max(covered, start + length);
      }
      expect(covered).toBe(end);
    }
  });

  it("does not upscale or re-encode a small image", async () => {
    const image = source(500, 200);
    const original = { data: "original", mimeType: "image/png" };
    expect(await prepareImageForSend(original, 50, async () => image)).toEqual([original]);
    expect(image.encode).not.toHaveBeenCalled();
    expect(image.release).toHaveBeenCalledOnce();
  });

  it("converts unsupported wire formats even when the original is small", async () => {
    const image = source(500, 200);
    const result = await prepareImageForSend(
      { data: "original", mimeType: "image/avif" },
      50,
      async () => image,
    );
    expect(result).toEqual([{ data: "encoded", mimeType: "image/png" }]);
    expect(image.encode).toHaveBeenCalledOnce();
    expect(image.release).toHaveBeenCalledOnce();
  });

  it("tries lossless output before high-quality compression and preserves MIME", async () => {
    const image = source(1800, 1000);
    image.encode = vi.fn(async (_region, mimeType) => ({
      data: mimeType === "image/png" ? "A".repeat(MAX_IMAGE_BASE64 + 4) : "small-webp",
      mimeType,
    }));
    const result = await prepareImageForSend(
      {
        data: "A".repeat(MAX_IMAGE_BASE64 + 4),
        mimeType: "image/png",
      },
      50,
      async () => image,
    );
    expect(result[0].data.length).toBeLessThanOrEqual(MAX_IMAGE_BASE64);
    expect(result).toEqual([{ data: "small-webp", mimeType: "image/webp" }]);
    expect(vi.mocked(image.encode).mock.calls.map(([, mime, quality]) => [mime, quality])).toEqual([
      ["image/png", 1],
      ["image/webp", 0.94],
    ]);
  });

  it("splits an incompressible region instead of reducing readability or quality indefinitely", async () => {
    const image = source(1800, 1000);
    image.encode = vi.fn(async (region, mimeType) => ({
      data: region.width > 1100 ? "A".repeat(MAX_IMAGE_BASE64 + 4) : "detail",
      mimeType,
    }));
    const result = await prepareImageForSend(
      {
        data: "A".repeat(MAX_IMAGE_BASE64 + 4),
        mimeType: "image/png",
      },
      50,
      async () => image,
    );
    expect(result).toHaveLength(2);
    const calls = vi.mocked(image.encode).mock.calls;
    expect(Math.min(...calls.map(([, , quality]) => quality))).toBe(0.82);
    for (const [region] of calls) {
      expect(region.outputWidth).toBe(region.width);
      expect(region.outputHeight).toBe(region.height);
    }
    expect(result[0].description).toContain("x=0");
    expect(result[1].description).toContain("x=836");
  });

  it("keeps the overview plus explicit coordinates for a long image", async () => {
    const image = source(800, 6000);
    const result = await prepareImageForSend(
      { data: "original", mimeType: "image/png" },
      50,
      async () => image,
    );
    expect(result[0].description).toContain("overview");
    expect(
      result.slice(1).every((item) => item.description?.includes("original pixel resolution")),
    ).toBe(true);
    expect(image.release).toHaveBeenCalledOnce();
  });

  it("fails before rendering when the image needs too many readable parts", async () => {
    const image = source(800, 1000000);
    await expect(
      prepareImageForSend({ data: "original", mimeType: "image/png" }, 50, async () => image),
    ).rejects.toThrow("too many readable parts");
    expect(image.encode).not.toHaveBeenCalled();
    expect(image.release).toHaveBeenCalledOnce();
  });

  it.each(["image/png", "image/jpeg", "image/webp"])(
    "checks oversized %s headers before bitmap decode",
    async (mimeType) => {
      const header = Buffer.alloc(40);
      if (mimeType === "image/png") {
        Buffer.from("89504e470d0a1a0a", "hex").copy(header);
        header.writeUInt32BE(60000, 16);
        header.writeUInt32BE(60000, 20);
        header.write("IDAT", 12);
      } else if (mimeType === "image/jpeg") {
        header[0] = 0xff;
        header[1] = 0xd8;
        header[2] = 0xff;
        header[3] = 0xc0;
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
        prepareImageForSend({ data: header.toString("base64"), mimeType }, 50, open),
      ).rejects.toThrow("too many readable parts");
      expect(open).not.toHaveBeenCalled();
    },
  );

  it("releases the source on encoding failure", async () => {
    const image = source(800, 6000);
    image.encode = vi.fn(async () => {
      throw new Error("failed");
    });
    await expect(
      prepareImageForSend({ data: "original", mimeType: "image/png" }, 50, async () => image),
    ).rejects.toThrow("failed");
    expect(image.release).toHaveBeenCalledOnce();
  });

  it.each(["image/gif", "image/webp", "image/png"])("preserves animated %s", async (mimeType) => {
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
    expect(await prepareImageForSend(original, 50, open)).toEqual([original]);
    expect(open).not.toHaveBeenCalled();
  });

  it("sends layout as text context without leaking client-only fields into image RPCs", () => {
    const payload = imageSendPayload(
      [
        { data: "one", mimeType: "image/png", description: "original image 1: overview" },
        { data: "two", mimeType: "image/webp", description: "original image 1: detail x=0" },
      ],
      [{ type: "text", mimeType: "text/plain", text: "user context" }],
    );
    expect(payload.images).toEqual([
      { data: "one", mimeType: "image/png" },
      { data: "two", mimeType: "image/webp" },
    ]);
    expect(payload.attachments).toHaveLength(2);
    expect(payload.attachments?.[1]).toMatchObject({
      type: "text",
      text: expect.stringContaining("Image 2: original image 1: detail x=0"),
    });
  });
});
