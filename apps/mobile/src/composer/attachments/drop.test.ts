import { describe, expect, it } from "vitest";
import { droppedItemsToSelectedFiles } from "./drop";

describe("composer dropped attachments", () => {
  it("turns non-image browser files into selected files and leaves raster images for image handling", async () => {
    const jsonFile = new File([JSON.stringify({ ok: true })], "config.json", {
      type: "application/json",
    });
    const imageFile = new File([new Uint8Array([0])], "screen.png", { type: "image/png" });

    const files = droppedItemsToSelectedFiles([
      { kind: "web-file", file: jsonFile },
      { kind: "web-file", file: imageFile },
    ]);

    expect(files.map(({ fileName, mimeType }) => ({ fileName, mimeType }))).toEqual([
      { fileName: "config.json", mimeType: "application/json" },
    ]);
    expect(await files[0]?.readBytes()).toEqual(new Uint8Array(await jsonFile.arrayBuffer()));
  });
});
