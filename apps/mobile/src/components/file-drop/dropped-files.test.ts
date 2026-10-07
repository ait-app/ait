import { describe, expect, it, vi } from "vitest";
import { splitDroppedFiles } from "./dropped-files";

function transfer(entries: { file: File; directory?: boolean }[]): DataTransfer {
  return {
    files: entries.map(({ file }) => file),
    items: entries.map(({ file, directory }) => ({
      kind: "file",
      getAsFile: () => file,
      webkitGetAsEntry: () => ({ isDirectory: Boolean(directory), name: file.name }),
    })),
  } as unknown as DataTransfer;
}

describe("directory drops", () => {
  it("separates folders from both file and image uploads in a mixed drop", () => {
    const directory = new File([], "photos.png");
    const image = new File(["image"], "image.png", { type: "image/png" });
    const document = new File(["hello"], "notes.txt");
    const getPathForFile = vi.fn(() => "/home/user/课程 作业/photos.png");
    expect(
      splitDroppedFiles(
        transfer([{ file: directory, directory: true }, { file: image }, { file: document }]),
        { webUtils: { getPathForFile } },
      ),
    ).toEqual({
      files: [image, document],
      directoryPaths: ["/home/user/课程 作业/photos.png"],
    });
    expect(getPathForFile).toHaveBeenCalledExactlyOnceWith(directory);
  });

  it("keeps legacy Windows paths when the modern desktop bridge is unavailable", () => {
    const directory = new File([], "Work Files");
    Object.defineProperty(directory, "path", { value: "C:\\Users\\me\\Work Files" });
    const getPathForFile = vi.fn(() => {
      throw new Error("unavailable");
    });
    expect(
      splitDroppedFiles(transfer([{ file: directory, directory: true }]), {
        webUtils: { getPathForFile },
      }),
    ).toEqual({ files: [], directoryPaths: ["C:\\Users\\me\\Work Files"] });
  });

  it("does not try to read a browser directory when its full path is hidden", () => {
    const directory = new File([], "folder");
    expect(splitDroppedFiles(transfer([{ file: directory, directory: true }]), null)).toEqual({
      files: [],
      directoryPaths: ["folder"],
    });
  });

  it("preserves file-only transfers without entry support", () => {
    const file = new File(["text"], "file.txt");
    expect(splitDroppedFiles({ files: [file] } as unknown as DataTransfer, null)).toEqual({
      files: [file],
      directoryPaths: [],
    });
    expect(splitDroppedFiles(null, null)).toEqual({ files: [], directoryPaths: [] });
  });
});
