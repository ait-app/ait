import { ImageManipulator, SaveFormat } from "expo-image-manipulator";
import { File } from "expo-file-system";
import type { EncodedSendImage, ImageDimensions, SendImageSource } from "./image-send";

export async function openSendImage(uri: string): Promise<SendImageSource> {
  const loader = ImageManipulator.manipulate(uri);
  let original: Awaited<ReturnType<typeof loader.renderAsync>>;
  try {
    original = await loader.renderAsync();
  } finally {
    loader.release();
  }
  return {
    width: original.width,
    height: original.height,
    async encode(
      size: ImageDimensions,
      mimeType: string,
      quality: number,
    ): Promise<EncodedSendImage> {
      const context = ImageManipulator.manipulate(original);
      let rendered: Awaited<ReturnType<typeof context.renderAsync>> | undefined;
      let savedUri: string | undefined;
      try {
        if (size.width !== original.width || size.height !== original.height) {
          context.resize(size);
        }
        rendered = await context.renderAsync();
        const format =
          mimeType === "image/png"
            ? SaveFormat.PNG
            : mimeType === "image/webp"
              ? SaveFormat.WEBP
              : SaveFormat.JPEG;
        const saved = await rendered.saveAsync({ format, compress: quality, base64: true });
        savedUri = saved.uri;
        if (!saved.base64) throw new Error("Image encoding failed.");
        return { data: saved.base64, mimeType };
      } finally {
        rendered?.release();
        context.release();
        if (savedUri) new File(savedUri).delete();
      }
    },
    release() {
      original.release();
    },
  };
}
