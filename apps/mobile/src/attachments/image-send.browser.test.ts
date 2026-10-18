import { describe, expect, it } from "vitest";
import { prepareImageForSend } from "./image-send";
import { openSendImage } from "./image-send-codec.web";
import { blobToBase64 } from "./utils";

function canvas(width: number, height: number): HTMLCanvasElement {
  const value = document.createElement("canvas");
  value.width = width;
  value.height = height;
  return value;
}

async function original(value: HTMLCanvasElement) {
  const blob = await new Promise<Blob>((resolve) =>
    value.toBlob((result) => resolve(result!), "image/png"),
  );
  return { data: await blobToBase64(blob), mimeType: "image/png" };
}

async function decode(data: string, mimeType: string): Promise<HTMLImageElement> {
  const image = new Image();
  image.src = `data:${mimeType};base64,${data}`;
  await image.decode();
  return image;
}

describe("real browser image send", () => {
  it.each([
    [600, 9000],
    [9000, 600],
  ])("keeps all four corners of a %ix%i image in a single image", async (width, height) => {
    const input = canvas(width, height);
    const context = input.getContext("2d")!;
    context.fillStyle = "white";
    context.fillRect(0, 0, width, height);
    context.fillStyle = "black";
    context.font = "16px monospace";
    context.fillText("Readable screenshot text", 10, 25);
    const corners = [
      [0, 0, "red"],
      [width - 40, 0, "lime"],
      [0, height - 40, "blue"],
      [width - 40, height - 40, "magenta"],
    ] as const;
    for (const [x, y, color] of corners) {
      context.fillStyle = color;
      context.fillRect(x, y, 40, 40);
    }
    const raw = await original(input);
    // Production limits preserve even extreme aspect ratios byte-for-byte.
    expect(await prepareImageForSend(raw)).toBe(raw);
    const budget = Math.floor(raw.data.length / 2);
    const result = await prepareImageForSend(raw, undefined, budget);
    expect(Array.isArray(result)).toBe(false);
    expect(result.data.length).toBeLessThanOrEqual(budget);
    const image = await decode(result.data, result.mimeType);
    expect(Math.abs(image.naturalWidth - (image.naturalHeight * width) / height)).toBeLessThan(2);
    const output = canvas(image.naturalWidth, image.naturalHeight);
    const pixels = output.getContext("2d")!;
    pixels.drawImage(image, 0, 0);
    const expected = [
      [255, 0, 0],
      [0, 255, 0],
      [0, 0, 255],
      [255, 0, 255],
    ];
    for (const [index, [x, y]] of corners.entries()) {
      const actual = pixels.getImageData(
        Math.floor(((x + 20) * output.width) / width),
        Math.floor(((y + 20) * output.height) / height),
        1,
        1,
      ).data;
      for (let channel = 0; channel < 3; channel++) {
        expect(Math.abs(actual[channel] - expected[index][channel])).toBeLessThan(30);
      }
      expect(actual[3]).toBe(255);
    }
    input.width = input.height = output.width = output.height = 0;
  });

  it("preserves alpha when reducing a large noisy PNG", async () => {
    const input = canvas(1500, 1000);
    const context = input.getContext("2d")!;
    const pixels = context.createImageData(input.width, input.height);
    let seed = 42;
    for (let index = 0; index < pixels.data.length; index += 4) {
      seed = (Math.imul(seed, 1664525) + 1013904223) | 0;
      pixels.data[index] = seed & 255;
      pixels.data[index + 1] = (seed >>> 8) & 255;
      pixels.data[index + 2] = (seed >>> 16) & 255;
      pixels.data[index + 3] = 255;
    }
    context.putImageData(pixels, 0, 0);
    context.clearRect(0, 0, 20, 20);
    const raw = await original(input);
    const budget = 4 * Math.ceil((1024 * 1024) / 3);
    expect(raw.data.length).toBeGreaterThan(budget);
    expect(await prepareImageForSend(raw)).toBe(raw);
    const result = await prepareImageForSend(raw, undefined, budget);
    expect(result.data.length).toBeLessThanOrEqual(budget);
    const image = await decode(result.data, result.mimeType);
    const output = canvas(image.naturalWidth, image.naturalHeight);
    const out = output.getContext("2d")!;
    out.drawImage(image, 0, 0);
    expect(out.getImageData(5, 5, 1, 1).data[3]).toBe(0);
    input.width = input.height = output.width = output.height = 0;
  });

  it("resizes a complete long image in the browser codec", async () => {
    const input = canvas(500, 12000);
    const image = await openSendImage(input.toDataURL());
    try {
      const encoded = await image.encode({ width: 250, height: 6000 }, "image/png", 1);
      const result = await decode(encoded.data, encoded.mimeType);
      expect([result.naturalWidth, result.naturalHeight]).toEqual([250, 6000]);
    } finally {
      image.release();
      input.width = input.height = 0;
    }
  });
});
