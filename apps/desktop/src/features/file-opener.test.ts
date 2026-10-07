import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { openDirectoryLink, openPreviewFile } from "./file-opener";
import { dialog, shell } from "electron";

vi.mock("electron", () => ({
  shell: { openPath: vi.fn(async () => "") },
  dialog: {
    showMessageBox: vi.fn(async () => ({ response: 0 })),
    showOpenDialog: vi.fn(async () => ({ canceled: true, filePaths: [] })),
  },
}));
const roots: string[] = [];
afterEach(async () => {
  vi.unstubAllEnvs();
  vi.clearAllMocks();
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "ait-file-opener-"));
  roots.push(root);
  vi.stubEnv("AIT_SERVER_DATA_DIR", root);
  await mkdir(path.join(root, "desktop-attachments"));
  const file = path.join(root, "desktop-attachments", "report.pdf");
  await writeFile(file, "%PDF-1.4");
  return { root, file };
}

describe("file and directory opening", () => {
  it("opens downloaded PDFs through the system default", async () => {
    const { file } = await fixture();
    await openPreviewFile(file);
    expect(shell.openPath).toHaveBeenCalledWith(file);
    expect(dialog.showOpenDialog).not.toHaveBeenCalled();
  });
  it("offers an application picker when the default opener fails and respects cancellation", async () => {
    const { file } = await fixture();
    vi.mocked(shell.openPath).mockResolvedValueOnce("No application");
    await expect(openPreviewFile(file)).resolves.toBeUndefined();
    expect(dialog.showOpenDialog).toHaveBeenCalledTimes(1);
  });
  it("opens directories before file preview and leaves regular files to the internal viewer", async () => {
    const { root, file } = await fixture();
    expect(await openDirectoryLink({ path: "desktop-attachments", cwd: root })).toBe(true);
    expect(shell.openPath).toHaveBeenCalledWith(path.join(root, "desktop-attachments"));
    expect(await openDirectoryLink({ path: file, cwd: root })).toBe(false);
  });
  it("rejects paths and symlinks outside managed preview storage", async () => {
    const { root } = await fixture();
    const outside = path.join(root, "secret");
    await writeFile(outside, "private");
    await expect(openPreviewFile(outside)).rejects.toThrow("downloaded preview");
    const link = path.join(root, "desktop-attachments", "link");
    await symlink(outside, link);
    await expect(openPreviewFile(link)).rejects.toThrow("downloaded preview");
    expect(shell.openPath).not.toHaveBeenCalled();
  });
});

it("requires explicit confirmation before opening executable previews", async () => {
  const { root } = await fixture();
  const file = path.join(root, "desktop-attachments", "PROGRAM.EXE");
  await writeFile(file, "inert test data");
  await openPreviewFile(file);
  expect(dialog.showMessageBox).toHaveBeenCalledWith(
    expect.objectContaining({ defaultId: 0, cancelId: 0 }),
  );
  expect(shell.openPath).not.toHaveBeenCalled();
  vi.mocked(dialog.showMessageBox).mockResolvedValueOnce({ response: 1, checkboxChecked: false });
  await openPreviewFile(file);
  expect(shell.openPath).toHaveBeenCalledWith(file);
});
