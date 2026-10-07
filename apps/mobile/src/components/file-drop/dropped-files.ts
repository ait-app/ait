import type { DesktopHostBridge } from "@/desktop/host";

/** Read drag entries synchronously, before the browser clears its drag data store. */
export function splitDroppedFiles(
  transfer: DataTransfer | null,
  bridge: DesktopHostBridge | null,
): { files: File[]; directoryPaths: string[] } {
  const files: File[] = [];
  const directoryPaths: string[] = [];
  const items = Array.from(transfer?.items ?? []).filter((item) => item.kind === "file");
  if (items.length === 0) return { files: Array.from(transfer?.files ?? []), directoryPaths };

  for (const item of items) {
    const entry = item.webkitGetAsEntry?.();
    const file = item.getAsFile();
    if (entry?.isDirectory) {
      let path: string | undefined;
      if (file) {
        try {
          path = bridge?.webUtils?.getPathForFile?.(file);
        } catch {
          // Older Electron builds expose File.path instead of webUtils.
        }
        const legacyPath: unknown = Reflect.get(file, "path");
        if (!path && typeof legacyPath === "string") path = legacyPath;
      }
      // Browsers conceal absolute paths. Keep the folder name rather than reading it as a file.
      directoryPaths.push(path || entry.name);
    } else if (file) {
      files.push(file);
    }
  }
  return { files, directoryPaths };
}
