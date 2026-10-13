import { describe, expect, it } from "vitest";
import { prepareImageForSend, MAX_IMAGE_BASE64, type ImageRegion } from "./image-send";
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
  ])("keeps the far edge and pixel resolution of %ix%i", async (width, height) => {
    const input = canvas(width, height);
    const context = input.getContext("2d")!;
    context.fillStyle = "white";
    context.fillRect(0, 0, width, height);
    context.fillStyle = "red";
    context.fillRect(width - 30, height - 30, 30, 30);
    context.fillStyle = "black";
    context.font = "16px monospace";
    context.fillText("Readable screenshot text", 10, 25);
    const result = await prepareImageForSend(await original(input), 50);
    expect(result.length).toBeGreaterThan(2);
    expect(result[0].description).toContain("overview");
    const last = await decode(result.at(-1)!.data, result.at(-1)!.mimeType);
    expect(Math.max(last.naturalWidth, last.naturalHeight)).toBeLessThanOrEqual(2048);
    expect(Math.min(last.naturalWidth, last.naturalHeight)).toBe(600);
    const output = canvas(last.naturalWidth, last.naturalHeight);
    const pixels = output.getContext("2d")!;
    pixels.drawImage(last, 0, 0);
    expect([...pixels.getImageData(output.width - 10, output.height - 10, 1, 1).data]).toEqual([
      255, 0, 0, 255,
    ]);
    expect(result.every((image) => image.data.length <= MAX_IMAGE_BASE64)).toBe(true);
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
    expect(raw.data.length).toBeGreaterThan(MAX_IMAGE_BASE64);
    const result = await prepareImageForSend(raw, 50);
    expect(result.reduce((sum, item) => sum + item.data.length, 0)).toBeLessThan(raw.data.length);
    const image = await decode(result[0].data, result[0].mimeType);
    const output = canvas(image.naturalWidth, image.naturalHeight);
    const out = output.getContext("2d")!;
    out.drawImage(image, 0, 0);
    expect(out.getImageData(5, 5, 1, 1).data[3]).toBe(0);
    input.width = input.height = output.width = output.height = 0;
  });

  it("uses only bounded canvases for crops and the overview", async () => {
    const input = canvas(500, 12000);
    const image = await openSendImage(input.toDataURL());
    try {
      const region: ImageRegion = {
        x: 0,
        y: 10000,
        width: 500,
        height: 2000,
        outputWidth: 500,
        outputHeight: 2000,
      };
      const encoded = await image.encode(region, "image/png", 1);
      const result = await decode(encoded.data, encoded.mimeType);
      expect([result.naturalWidth, result.naturalHeight]).toEqual([500, 2000]);
    } finally {
      image.release();
      input.width = input.height = 0;
    }
  });
});
