import type { DownloadItem, WebContents } from "electron";
import path from "node:path";

/** Own direct Host downloads without navigating the privileged application window. */
export class DirectDownloadManager {
  private active = new Set<() => void>();

  /** Stream a one-use file URL through Electron and resolve only after it reaches disk. */
  async download(owner: WebContents, args: Record<string, unknown> = {}): Promise<void> {
    const url = this.url(args.url);
    if (typeof args.downloadId !== "string" || !args.downloadId || owner.isDestroyed())
      throw new Error("Invalid download request.");
    if (this.active.size >= 4) throw new Error("Too many concurrent downloads.");
    const downloadId = args.downloadId;
    const fileName = path.basename(typeof args.fileName === "string" ? args.fileName : "download");
    await new Promise<void>((resolve, reject) => {
      let item: DownloadItem | undefined;
      let settled = false;
      let lastProgress = 0;
      let timer: ReturnType<typeof setTimeout>;
      const finish = (error?: Error) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.active.delete(cancel);
        owner.session.removeListener("will-download", started);
        owner.removeListener("destroyed", cancel);
        owner.removeListener("did-start-navigation", navigated);
        item?.removeListener("updated", progress);
        item?.removeListener("done", done);
        if (error) {
          item?.cancel();
          reject(error);
        } else resolve();
      };
      const cancel = () => finish(new Error("Download cancelled."));
      const navigated = (_event: unknown, _url: string, sameDocument: boolean, main: boolean) => {
        if (main && !sameDocument) cancel();
      };
      const progress = () => {
        clearTimeout(timer);
        timer = setTimeout(() => finish(new Error("Download stalled.")), 30_000);
        if (!item || Date.now() - lastProgress < 200 || owner.isDestroyed()) return;
        lastProgress = Date.now();
        owner.send("paseo:event:direct-download-progress", {
          id: downloadId,
          bytesWritten: item.getReceivedBytes(),
          totalBytes: item.getTotalBytes(),
        });
      };
      const done = (_event: unknown, state: string) =>
        finish(state === "completed" ? undefined : new Error("Download interrupted."));
      const started = (_event: unknown, candidate: DownloadItem, source: WebContents) => {
        if (source !== owner || !candidate.getURLChain().includes(url)) return;
        clearTimeout(timer);
        item = candidate;
        // Electron streams to its temporary download and presents the native save dialog.
        // User time in that dialog has no timeout and does not age an unconsumed file token.
        item.setSaveDialogOptions({ defaultPath: fileName });
        owner.session.removeListener("will-download", started);
        item.on("updated", progress);
        item.once("done", done);
      };
      this.active.add(cancel);
      owner.session.on("will-download", started);
      owner.once("destroyed", cancel);
      owner.on("did-start-navigation", navigated);
      timer = setTimeout(() => finish(new Error("Download connection timed out.")), 30_000);
      try {
        owner.downloadURL(url);
      } catch {
        finish(new Error("Download could not start."));
      }
    });
  }

  private url(value: unknown): string {
    let url: URL;
    try {
      url = new URL(typeof value === "string" ? value : "");
    } catch {
      throw new Error("Invalid download URL.");
    }
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.pathname !== "/api/files/download" ||
      !url.searchParams.get("token") ||
      [...url.searchParams.keys()].some((key) => key !== "token") ||
      url.hash
    )
      throw new Error("Invalid download URL.");
    return url.href;
  }
}
