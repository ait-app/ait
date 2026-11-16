import type { AttachmentMetadata } from "@/attachments/types";
import { encodeAttachmentsForSend } from "@/attachments/service";
import type { EncodedSendImage, ImageSendContext } from "@/attachments/image-send";

type ImageInput = AttachmentMetadata;

export async function encodeImages(
  images?: ImageInput[],
  context?: ImageSendContext,
): Promise<EncodedSendImage[] | undefined> {
  return await encodeAttachmentsForSend(images, context);
}
