import { Buffer } from "buffer";
import type { AgentAttachment } from "@ait/protocol/messages";
import { openSendImage } from "./image-send-codec";

export const MAX_SEND_IMAGES = 50;
// Wire admission limits, measured after base64 encoding / JSON serialization.
export const MAX_ORIGINAL_IMAGE_BASE64 = 32 * 1024 * 1024;
export const MAX_IMAGE_PROMPT_BYTES = 63 * 1024 * 1024;
const MAX_DECODE_PIXELS = 80 * 1024 * 1024;
// Bound temporary output surfaces while retaining the entire image.
const MAX_ENCODE_PIXELS = 16 * 1024 * 1024;
const MAX_ENCODE_EDGE = 16384;
const MAX_RESIZE_ATTEMPTS = 8;

export interface EncodedSendImage {
  data: string;
  mimeType: string;
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

export interface ImageDimensions {
  width: number;
  height: number;
}

export interface SendImageSource {
  width: number;
  height: number;
  encode(size: ImageDimensions, mimeType: string, quality: number): Promise<EncodedSendImage>;
  release(): void;
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

async function encodeWholeImage(
  source: SendImageSource,
  size: ImageDimensions,
  originalMimeType: string,
  maxBase64Bytes: number,
): Promise<EncodedSendImage> {
  let best: EncodedSendImage | undefined;
  // PNG preserves screenshot text and alpha when its lossless encoding already fits.
  if (originalMimeType !== "image/jpeg") {
    best = await source.encode(size, "image/png", 1);
    if (best.data.length <= maxBase64Bytes) return best;
  }
  const mimeType = originalMimeType === "image/jpeg" ? "image/jpeg" : "image/webp";
  for (const quality of [0.94, 0.88, 0.82]) {
    const candidate = await source.encode(size, mimeType, quality);
    if (!best || candidate.data.length < best.data.length) best = candidate;
    if (best.data.length <= maxBase64Bytes || candidate.mimeType !== mimeType) break;
  }
  return best!;
}

function validateDimensions({ width, height }: ImageDimensions): void {
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width < 1 || height < 1) {
    throw new Error("The image dimensions could not be read.");
  }
  if (width * height > MAX_DECODE_PIXELS) {
    throw new Error("This image is too large to process safely. Send a smaller image.");
  }
}

/** One original always produces one complete image; only its encoding and size may change. */
export async function prepareImageForSend(
  original: EncodedSendImage,
  open: (uri: string) => Promise<SendImageSource> = openSendImage,
  maxBase64Bytes: number = MAX_ORIGINAL_IMAGE_BASE64,
): Promise<EncodedSendImage> {
  if (!Number.isSafeInteger(maxBase64Bytes) || maxBase64Bytes < 1) {
    throw new Error("The image byte budget must be positive.");
  }
  const oversized = original.data.length > maxBase64Bytes;
  // Geometry never starts compression. Supported files within their byte budget
  // bypass even header parsing and bitmap decoding, including very long images.
  if (!oversized && supportsSendImage(original.mimeType)) return original;
  if (isAnimatedSendImage(original)) {
    if (oversized) {
      throw new Error("This animated image is too large. Send a smaller file.");
    }
    return original;
  }
  // Preflight headers before loading a huge bitmap. Decoded orientation can differ;
  // the actual codec dimensions still own the final resize ratio.
  const dimensions = headerDimensions(original);
  if (dimensions) validateDimensions(dimensions);
  const source = await open(`data:${original.mimeType};base64,${original.data}`);
  try {
    validateDimensions(source);
    let scale = Math.min(
      1,
      MAX_ENCODE_EDGE / Math.max(source.width, source.height),
      Math.sqrt(MAX_ENCODE_PIXELS / (source.width * source.height)),
    );
    for (let attempt = 0; attempt <= MAX_RESIZE_ATTEMPTS; attempt++) {
      const size = {
        width: Math.max(1, Math.floor(source.width * scale)),
        height: Math.max(1, Math.floor(source.height * scale)),
      };
      const encoded = await encodeWholeImage(source, size, original.mimeType, maxBase64Bytes);
      if (encoded.data.length <= maxBase64Bytes) return encoded;
      if (size.width === 1 && size.height === 1) break;
      scale *= Math.min(0.75, Math.sqrt(maxBase64Bytes / encoded.data.length) * 0.9);
    }
    throw new Error(
      "This image cannot fit the message budget. Send a smaller image or fewer images.",
    );
  } finally {
    source.release();
  }
}

/** Count the actual prompt JSON, including UTF-8 text and user attachments.
 * Standard base64 needs no JSON escaping, so avoid duplicating large data strings.
 */
export function imagePromptBytes(images: EncodedSendImage[], context: ImageSendContext): number {
  const envelope = Buffer.byteLength(
    JSON.stringify({
      text: context.text ?? "",
      images: [],
      attachments: context.attachments ?? [],
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
