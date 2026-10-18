import type { EncodedSendImage, ImageDimensions, SendImageSource } from "./image-send";
import { blobToBase64 } from "./utils";

export async function openSendImage(uri: string): Promise<SendImageSource> {
  const image = new Image();
  image.src = uri;
  try {
    await image.decode();
  } catch {
    image.src = "";
    throw new Error("The image could not be read. Remove it and attach it again.");
  }
  return {
    width: image.naturalWidth,
    height: image.naturalHeight,
    async encode(
      size: ImageDimensions,
      mimeType: string,
      quality: number,
    ): Promise<EncodedSendImage> {
      const canvas = document.createElement("canvas");
      canvas.width = size.width;
      canvas.height = size.height;
      try {
        const context = canvas.getContext("2d");
        if (!context) throw new Error("Image processing is unavailable.");
        context.imageSmoothingEnabled = true;
        context.imageSmoothingQuality = "high";
        context.drawImage(image, 0, 0, canvas.width, canvas.height);
        const blob = await new Promise<Blob>((resolve, reject) => {
          canvas.toBlob(
            (value) => (value ? resolve(value) : reject(new Error("Image encoding failed."))),
            mimeType,
            quality,
          );
        });
        // Browsers that cannot encode WebP can return PNG; report the actual format.
        return { data: await blobToBase64(blob), mimeType: blob.type };
      } finally {
        canvas.width = 0;
        canvas.height = 0;
      }
    },
    release() {
      image.src = "";
    },
  };
}
