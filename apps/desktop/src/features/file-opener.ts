import { spawn } from "node:child_process";
import { access, realpath, stat } from "node:fs/promises";
import { constants } from "node:fs";
import os from "node:os";
import path from "node:path";
import { dialog, shell } from "electron";
import { resolveDesktopDaemonHome } from "../daemon/rust-daemon.js";

/** Open a downloaded preview on this computer, falling back to an application picker. */
export async function openPreviewFile(value: unknown): Promise<void> {
  if (typeof value !== "string" || !path.isAbsolute(value)) {
    throw new Error("A local preview file is required.");
  }
  const root = await realpath(
    path.join(resolveDesktopDaemonHome(process.env), "desktop-attachments"),
  );
  const target = await realpath(value);
  if (!target.startsWith(`${root}${path.sep}`) || !(await stat(target)).isFile()) {
    throw new Error("The file must be a downloaded preview.");
  }
  await openWithApplication(target);
}

/** Open a directory link; regular files continue through the internal preview. */
export async function openDirectoryLink(value: unknown): Promise<boolean> {
  if (!value || typeof value !== "object") throw new Error("A directory target is required.");
  const { path: rawPath, cwd } = value as { path?: unknown; cwd?: unknown };
  if (typeof rawPath !== "string" || typeof cwd !== "string" || !path.isAbsolute(cwd)) {
    throw new Error("Invalid directory target.");
  }
  const expanded =
    rawPath === "~"
      ? os.homedir()
      : rawPath.startsWith("~/")
        ? path.join(os.homedir(), rawPath.slice(2))
        : rawPath;
  const target = path.resolve(cwd, expanded);
  const entry = await stat(target).catch(() => null);
  if (!entry?.isDirectory()) return false;
  await openWithApplication(target);
  return true;
}

async function openWithApplication(target: string): Promise<void> {
  const error = await shell.openPath(target);
  if (!error) return;
  const choice = await dialog.showOpenDialog({
    title: "Open with / 选择打开应用",
    properties: ["openFile"],
    ...(process.platform === "darwin"
      ? { defaultPath: "/Applications", filters: [{ name: "Applications", extensions: ["app"] }] }
      : {}),
  });
  const application = choice.filePaths[0];
  if (choice.canceled || !application) return;
  if (process.platform !== "darwin") await access(application, constants.X_OK);
  await launchApplication(application, target);
}

function launchApplication(application: string, target: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const child =
      process.platform === "darwin"
        ? spawn("/usr/bin/open", ["-a", application, target], { stdio: "ignore" })
        : spawn(application, [target], { detached: true, stdio: "ignore" });
    child.once("error", reject);
    if (process.platform === "darwin") {
      child.once("exit", (code) =>
        code === 0 ? resolve() : reject(new Error("The application could not open the file.")),
      );
    } else {
      child.once("spawn", () => {
        child.unref();
        resolve();
      });
    }
  });
}
