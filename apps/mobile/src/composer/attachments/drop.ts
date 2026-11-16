import { getMimeTypeFromPath, isRasterImageFile } from "@/attachments/file-types";
import type { SelectedFile } from "@/attachments/selected-file";
import type { DroppedItem } from "@/components/file-drop/types";

export function droppedItemsToSelectedFiles(items: DroppedItem[]): SelectedFile[] {
  const files: SelectedFile[] = [];

  for (const item of items) {
    if (isRasterImageFile(item.file)) {
      continue;
    }
    const file = item.file;
    files.push({
      fileName: file.name,
      mimeType: file.type || getMimeTypeFromPath(file.name),
      readBytes: async () => new Uint8Array(await file.arrayBuffer()),
    });
  }

  return files;
}
