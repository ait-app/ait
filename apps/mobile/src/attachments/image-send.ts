import { Buffer } from "buffer";
import type { AgentAttachment } from "@ait/protocol/messages";
import { openSendImage } from "./image-send-codec";

export const MAX_SEND_IMAGES = 50;
// Wire admission limits, measured after base64 encoding / JSON serialization.
export const MAX_ORIGINAL_IMAGE_BASE64 = 32 * 1024 * 1024;
export const MAX_IMAGE_PROMPT_BYTES = 63 * 1024 * 1024;
// Encoding target only after the original exceeds a wire budget.
export const MAX_IMAGE_BASE64 = 4 * Math.ceil((1024 * 1024) / 3);
const TILE_EDGE = 2048;
const OVERLAP = 128;

export interface EncodedSendImage {
  data: string;
  mimeType: string;
  description?: string;
}

export interface ImageSendContext {
  text?: string;
  attachments?: AgentAttachment[];
  clientMessageId?: string;
  outputSchema?: unknown;
}

export function supportsSendImage(mimeType: string): boolean {
  return ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(mimeType);
}

export interface ImageRegion {
  x: number;
  y: number;
  width: number;
  height: number;
  outputWidth: number;
  outputHeight: number;
  overview?: boolean;
}

export interface SendImageSource {
  width: number;
  height: number;
  encode(region: ImageRegion, mimeType: string, quality: number): Promise<EncodedSendImage>;
  release(): void;
}

export function imagePartsError(): Error {
  return new Error("These images need too many readable parts. Send fewer images or crop them.");
}

function axisRegions(length: number): Array<{ start: number; length: number }> {
  const count = Math.max(1, Math.ceil((length - OVERLAP) / (TILE_EDGE - OVERLAP)));
  if (count > MAX_SEND_IMAGES) throw imagePartsError();
  if (count === 1) return [{ start: 0, length }];
  const size = Math.ceil((length + (count - 1) * OVERLAP) / count);
  return Array.from({ length: count }, (_, index) => ({
    start: index * (size - OVERLAP),
    length: Math.min(size, length - index * (size - OVERLAP)),
  }));
}

/** Keep detail crops at original resolution; only the overview is downsampled. */
export function planSendImage(width: number, height: number, maxParts: number): ImageRegion[] {
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width < 1 || height < 1) {
    throw new Error("The image dimensions could not be read.");
  }
  if (width <= TILE_EDGE && height <= TILE_EDGE) {
    return [{ x: 0, y: 0, width, height, outputWidth: width, outputHeight: height }];
  }
  const columns = axisRegions(width);
  const rows = axisRegions(height);
  if (columns.length * rows.length + 1 > maxParts) throw imagePartsError();
  const scale = Math.min(1, 1024 / Math.max(width, height));
  const regions: ImageRegion[] = [
    {
      x: 0,
      y: 0,
      width,
      height,
      outputWidth: Math.max(1, Math.round(width * scale)),
      outputHeight: Math.max(1, Math.round(height * scale)),
      overview: true,
    },
  ];
  for (const row of rows) {
    for (const column of columns) {
      regions.push({
        x: column.start,
        y: row.start,
        width: column.length,
        height: row.length,
        outputWidth: column.length,
        outputHeight: row.length,
      });
    }
  }
  return regions;
}

function base64Bytes(data: string, offset: number, length: number): Buffer {
  const block = Math.floor(offset / 3);
  return Buffer.from(
    data.slice(block * 4, Math.ceil((offset + length) / 3) * 4),
    "base64",
  ).subarray(offset - block * 3, offset - block * 3 + length);
}

function headerDimensions(image: EncodedSendImage): { width: number; height: number } | undefined {
  const header = base64Bytes(image.data, 0, 30);
  if (
    image.mimeType === "image/png" &&
    header.length >= 24 &&
    header.toString("hex", 0, 8) === "89504e470d0a1a0a"
  ) {
    return { width: header.readUInt32BE(16), height: header.readUInt32BE(20) };
  }
  if (
    image.mimeType === "image/webp" &&
    header.length >= 30 &&
    header.toString("ascii", 0, 4) === "RIFF" &&
    header.toString("ascii", 8, 12) === "WEBP"
  ) {
    const type = header.toString("ascii", 12, 16);
    if (type === "VP8X")
      return {
        width: header.readUIntLE(24, 3) + 1,
        height: header.readUIntLE(27, 3) + 1,
      };
    if (type === "VP8 ")
      return {
        width: header.readUInt16LE(26) & 0x3fff,
        height: header.readUInt16LE(28) & 0x3fff,
      };
    if (type === "VP8L") {
      const bits = header.readUInt32LE(21);
      return { width: (bits & 0x3fff) + 1, height: ((bits >>> 14) & 0x3fff) + 1 };
    }
  }
  if (image.mimeType !== "image/jpeg" || header[0] !== 0xff || header[1] !== 0xd8) return;
  let offset = 2;
  for (let segment = 0; segment < 4096; segment++) {
    const marker = base64Bytes(image.data, offset, 4);
    if (marker.length < 4 || marker[0] !== 0xff) return;
    if (marker[1] === 0xff) {
      offset++;
      continue;
    }
    if (marker[1] === 0xda || marker[1] === 0xd9) return;
    const length = marker.readUInt16BE(2);
    if (length < 2) return;
    if (
      [0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf].includes(
        marker[1],
      )
    ) {
      const dimensions = base64Bytes(image.data, offset + 4, 5);
      if (dimensions.length < 5) return;
      return { width: dimensions.readUInt16BE(3), height: dimensions.readUInt16BE(1) };
    }
    offset += 2 + length;
  }
}

/** Preserve animation instead of quietly replacing an attachment with its first frame. */
export function isAnimatedSendImage(image: EncodedSendImage): boolean {
  if (image.mimeType === "image/gif") return true;
  if (image.mimeType === "image/webp") {
    const header = base64Bytes(image.data, 0, 21);
    return header.toString("ascii", 12, 16) === "VP8X" && (header[20] & 2) !== 0;
  }
  if (image.mimeType !== "image/png") return false;
  if (base64Bytes(image.data, 0, 8).toString("hex") !== "89504e470d0a1a0a") return false;
  let offset = 8;
  for (let count = 0; count < 4096; count++) {
    const header = base64Bytes(image.data, offset, 8);
    if (header.length < 8) return false;
    const type = header.toString("ascii", 4, 8);
    if (type === "acTL") return true;
    if (type === "IDAT" || type === "IEND") return false;
    offset += header.readUInt32BE(0) + 12;
  }
  return true;
}

async function encodeRegion(
  source: SendImageSource,
  region: ImageRegion,
  originalMimeType: string,
  maxBase64Bytes: number,
): Promise<EncodedSendImage> {
  let best: EncodedSendImage | undefined;
  // PNG preserves screenshot text and alpha when its lossless encoding already fits.
  if (originalMimeType !== "image/jpeg") {
    best = await source.encode(region, "image/png", 1);
    if (best.data.length <= maxBase64Bytes) return best;
  }
  const mimeType = originalMimeType === "image/jpeg" ? "image/jpeg" : "image/webp";
  for (const quality of [0.94, 0.88, 0.82]) {
    const candidate = await source.encode(region, mimeType, quality);
    if (!best || candidate.data.length < best.data.length) best = candidate;
    if (best.data.length <= maxBase64Bytes || candidate.mimeType !== mimeType) break;
  }
  return best!;
}

function splitRegion(region: ImageRegion): ImageRegion[] {
  const horizontal = region.width >= region.height;
  const length = horizontal ? region.width : region.height;
  if (length < 2) throw new Error("The image could not be compressed safely.");
  const middle = Math.floor(length / 2);
  const overlap = Math.min(OVERLAP / 2, Math.floor(length / 4));
  return [
    { start: 0, length: middle + overlap },
    {
      start: middle - overlap,
      length: length - middle + overlap,
    },
  ].map((part) => ({
    ...region,
    x: region.x + (horizontal ? part.start : 0),
    y: region.y + (horizontal ? 0 : part.start),
    width: horizontal ? part.length : region.width,
    height: horizontal ? region.height : part.length,
    outputWidth: horizontal ? part.length : region.width,
    outputHeight: horizontal ? region.height : part.length,
  }));
}

export async function prepareImageForSend(
  original: EncodedSendImage,
  maxParts: number,
  open: (uri: string) => Promise<SendImageSource> = openSendImage,
  maxBase64Bytes: number = MAX_ORIGINAL_IMAGE_BASE64,
): Promise<EncodedSendImage[]> {
  if (maxParts < 1) throw imagePartsError();
  const oversized = original.data.length > maxBase64Bytes;
  // Geometry never starts compression. Supported files within their byte budget
  // bypass even header parsing and bitmap decoding, including very long images.
  if (!oversized && supportsSendImage(original.mimeType)) return [original];
  if (isAnimatedSendImage(original)) {
    if (oversized) {
      throw new Error("This animated image is too large. Send a smaller file.");
    }
    return [original];
  }
  // Preflight headers before loading a huge bitmap. Decoded orientation can differ;
  // the actual codec dimensions still own the final crop plan.
  const dimensions = headerDimensions(original);
  if (dimensions) {
    if (oversized) planSendImage(dimensions.width, dimensions.height, maxParts);
    if (dimensions.width * dimensions.height > 80 * 1024 * 1024) {
      throw new Error("This image is too large to process safely. Crop it or send it in parts.");
    }
  }
  const source = await open(`data:${original.mimeType};base64,${original.data}`);
  try {
    // Format conversion alone also keeps the full original dimensions.
    const queue: ImageRegion[] = oversized
      ? planSendImage(source.width, source.height, maxParts)
      : [
          {
            x: 0,
            y: 0,
            width: source.width,
            height: source.height,
            outputWidth: source.width,
            outputHeight: source.height,
          },
        ];
    const result: EncodedSendImage[] = [];
    const outputLimit = oversized ? MAX_IMAGE_BASE64 : maxBase64Bytes;
    while (queue.length > 0) {
      const region = queue.shift()!;
      const encoded = await encodeRegion(source, region, original.mimeType, outputLimit);
      if (encoded.data.length > outputLimit) {
        if (region.overview) {
          region.outputWidth = Math.max(1, Math.floor(region.outputWidth / 2));
          region.outputHeight = Math.max(1, Math.floor(region.outputHeight / 2));
          if (region.outputWidth === 1 && region.outputHeight === 1) {
            throw new Error("The image could not be compressed safely.");
          }
          queue.unshift(region);
        } else {
          if (result.length + queue.length + 2 > maxParts) throw imagePartsError();
          queue.unshift(...splitRegion(region));
        }
        continue;
      }
      result.push({
        ...encoded,
        description: region.overview
          ? `overview of the full ${source.width}x${source.height} image`
          : `detail crop x=${region.x}, y=${region.y}, width=${region.width}, height=${region.height}; original pixel resolution`,
      });
    }
    if (result.length === 1) delete result[0].description;
    return result;
  } finally {
    source.release();
  }
}

/** Count the actual prompt JSON, including UTF-8 context and crop grouping notes.
 * Standard base64 needs no JSON escaping, so avoid duplicating large data strings.
 */
export function imagePromptBytes(images: EncodedSendImage[], context: ImageSendContext): number {
  const payload = imageSendPayload(images, context.attachments);
  const envelope = Buffer.byteLength(
    JSON.stringify({
      text: context.text ?? "",
      images: [],
      attachments: payload.attachments ?? [],
      clientMessageId: context.clientMessageId ?? null,
      outputSchema: context.outputSchema ?? null,
    }),
    "utf8",
  );
  return (
    envelope +
    images.reduce(
      (bytes, image, index) =>
        bytes +
        image.data.length +
        Buffer.byteLength(JSON.stringify({ data: "", mimeType: image.mimeType }), "utf8") +
        (index > 0 ? 1 : 0),
      0,
    )
  );
}

/** Strip client-only layout metadata and provide one model-facing grouping note. */
export function imageSendPayload(
  prepared: EncodedSendImage[] | undefined,
  attachments: AgentAttachment[] | undefined,
): {
  images: Array<{ data: string; mimeType: string }> | undefined;
  attachments: AgentAttachment[] | undefined;
} {
  const descriptions = prepared?.flatMap((image, index) =>
    image.description ? [`Image ${index + 1}: ${image.description}`] : [],
  );
  if (!descriptions?.length)
    return {
      images: prepared?.map(({ data, mimeType }) => ({ data, mimeType })),
      attachments,
    };
  if ((attachments?.length ?? 0) >= 32) {
    throw new Error(
      "There are too many attachments to describe the image parts. Send fewer attachments.",
    );
  }
  return {
    images: prepared?.map(({ data, mimeType }) => ({ data, mimeType })),
    attachments: [
      ...(attachments ?? []),
      {
        type: "text",
        mimeType: "text/plain",
        text: `Image layout: overviews show the full image; detail crops preserve readable pixels and overlap. Crop coordinates refer to the top-left of the original image.\n${descriptions.join("\n")}`,
      },
    ],
  };
}
