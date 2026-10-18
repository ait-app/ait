import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SendImageSource } from "./image-send";

const mocks = vi.hoisted(() => ({
  original: { width: 800, height: 12000, release: vi.fn() },
  rendered: { release: vi.fn(), saveAsync: vi.fn() },
  contexts: [] as Array<{
    crop: ReturnType<typeof vi.fn>;
    resize: ReturnType<typeof vi.fn>;
    renderAsync: ReturnType<typeof vi.fn>;
    release: ReturnType<typeof vi.fn>;
  }>,
  deleted: [] as string[],
}));

vi.mock("expo-image-manipulator", () => ({
  SaveFormat: { PNG: "png", WEBP: "webp", JPEG: "jpeg" },
  ImageManipulator: {
    manipulate: (input: unknown) => {
      const context = {
        crop: vi.fn(),
        resize: vi.fn(),
        release: vi.fn(),
        renderAsync: vi.fn(async () =>
          typeof input === "string" ? mocks.original : mocks.rendered,
        ),
      };
      mocks.contexts.push(context);
      return context;
    },
  },
}));

vi.mock("expo-file-system", () => ({
  File: class {
    constructor(private uri: string) {}
    delete() {
      mocks.deleted.push(this.uri);
    }
  },
}));

// Import the native implementation explicitly; Vitest otherwise selects the web extension.
const { openSendImage } = await vi.importActual<{
  openSendImage(uri: string): Promise<SendImageSource>;
}>("./image-send-codec.ts");

describe("native image send codec", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.contexts.length = 0;
    mocks.deleted.length = 0;
    mocks.rendered.saveAsync.mockResolvedValue({
      uri: "file:///cache/send.webp",
      base64: "encoded",
    });
  });

  it("encodes the complete image and releases native refs and transient cache files", async () => {
    const source = await openSendImage("data:image/png;base64,original");
    expect([source.width, source.height]).toEqual([800, 12000]);
    const result = await source.encode({ width: 800, height: 12000 }, "image/webp", 0.94);
    expect(result).toEqual({ data: "encoded", mimeType: "image/webp" });
    expect(mocks.contexts[1].crop).not.toHaveBeenCalled();
    expect(mocks.contexts[1].resize).not.toHaveBeenCalled();
    expect(mocks.rendered.saveAsync).toHaveBeenCalledWith({
      format: "webp",
      compress: 0.94,
      base64: true,
    });
    expect(mocks.deleted).toEqual(["file:///cache/send.webp"]);
    expect(mocks.rendered.release).toHaveBeenCalledOnce();
    expect(mocks.contexts.every((context) => context.release.mock.calls.length === 1)).toBe(true);
    source.release();
    expect(mocks.original.release).toHaveBeenCalledOnce();
  });

  it("resizes the entire original without cropping", async () => {
    const source = await openSendImage("original");
    await source.encode(
      {
        width: 68,
        height: 1024,
      },
      "image/png",
      1,
    );
    expect(mocks.contexts[1].crop).not.toHaveBeenCalled();
    expect(mocks.contexts[1].resize).toHaveBeenCalledWith({ width: 68, height: 1024 });
    source.release();
  });

  it("releases the transformation on encoding failure", async () => {
    const source = await openSendImage("original");
    mocks.rendered.saveAsync.mockRejectedValue(new Error("encoder failed"));
    await expect(source.encode({ width: 800, height: 12000 }, "image/png", 1)).rejects.toThrow(
      "encoder failed",
    );
    expect(mocks.rendered.release).toHaveBeenCalledOnce();
    expect(mocks.contexts[1].release).toHaveBeenCalledOnce();
    source.release();
  });
});
