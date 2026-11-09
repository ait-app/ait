import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  copyAttachmentFileToManagedStorage,
  createManagedAttachmentFileCoordinator,
  deleteManagedAttachmentFile,
  garbageCollectManagedAttachmentFiles,
  readManagedFileBase64,
  writeAttachmentBase64,
  writeAttachmentBytes,
} from "./attachments";

const originalPaseoHome = process.env.AIT_SERVER_DATA_DIR;
let testHome: string | null = null;

async function useTempPaseoHome(): Promise<string> {
  testHome = await mkdtemp(path.join(os.tmpdir(), "paseo-desktop-attachments-"));
  process.env.AIT_SERVER_DATA_DIR = testHome;
  return testHome;
}

function owner() {
  let destroyed = false;
  let onDestroyed = () => {};
  return {
    once: (_event: "destroyed", listener: () => void) => {
      onDestroyed = listener;
    },
    isDestroyed: () => destroyed,
    close: () => {
      destroyed = true;
      onDestroyed();
    },
  };
}

describe("desktop attachment files", () => {
  afterEach(async () => {
    if (originalPaseoHome === undefined) {
      delete process.env.AIT_SERVER_DATA_DIR;
    } else {
      process.env.AIT_SERVER_DATA_DIR = originalPaseoHome;
    }

    if (testHome) {
      await rm(testHome, { recursive: true, force: true });
      testHome = null;
    }
  });

  it("accepts dot-prefixed picker extensions for managed copies", async () => {
    const paseoHome = await useTempPaseoHome();
    const sourcePath = path.join(paseoHome, "report.md");
    await writeFile(sourcePath, "# Report\n");

    const result = await copyAttachmentFileToManagedStorage({
      attachmentId: "att_markdown",
      sourcePath,
      extension: ".md",
    });

    expect(result).toEqual({
      path: path.join(paseoHome, "desktop-attachments", "att_markdown.md"),
      byteSize: 9,
    });
    await expect(readFile(result.path, "utf8")).resolves.toBe("# Report\n");
  });

  it("normalizes legacy bare extensions for managed copies", async () => {
    const paseoHome = await useTempPaseoHome();
    const sourcePath = path.join(paseoHome, "report.md");
    await writeFile(sourcePath, "# Report\n");

    const result = await copyAttachmentFileToManagedStorage({
      attachmentId: "att_markdown_legacy",
      sourcePath,
      extension: "md",
    });

    expect(result).toEqual({
      path: path.join(paseoHome, "desktop-attachments", "att_markdown_legacy.md"),
      byteSize: 9,
    });
    await expect(readFile(result.path, "utf8")).resolves.toBe("# Report\n");
  });

  it("labels copies with unusual extensions as binary instead of failing", async () => {
    const paseoHome = await useTempPaseoHome();
    const sourcePath = path.join(paseoHome, "notes.backup_old");
    await writeFile(sourcePath, "kept");

    const result = await copyAttachmentFileToManagedStorage({
      attachmentId: "att_unusual",
      sourcePath,
      extension: ".backup_old",
    });

    expect(result.path).toBe(path.join(paseoHome, "desktop-attachments", "att_unusual.bin"));
    await expect(readFile(result.path, "utf8")).resolves.toBe("kept");
  });

  it("collects against the union of window references and newly created files", async () => {
    await useTempPaseoHome();
    const files = createManagedAttachmentFileCoordinator();
    const first = owner();
    const second = owner();
    const a = await files.writeBytes({ attachmentId: "first", bytes: [1] }, first);
    const b = await files.writeBytes({ attachmentId: "second", bytes: [2] }, second);

    // The second window has not reported its new attachment in a GC snapshot yet.
    expect(await files.garbageCollect({ referencedIds: ["first"] }, first)).toBe(0);
    await expect(readFile(b.path)).resolves.toEqual(Buffer.from([2]));
    expect(await files.garbageCollect({ referencedIds: ["second"] }, second)).toBe(0);
    expect(await files.garbageCollect({ referencedIds: [] }, first)).toBe(1);
    await expect(readFile(a.path)).rejects.toMatchObject({ code: "ENOENT" });
    await expect(readFile(b.path)).resolves.toEqual(Buffer.from([2]));
  });

  it("keeps a file created while an older reference snapshot is waiting to collect", async () => {
    await useTempPaseoHome();
    const started = Promise.withResolvers<void>();
    const writing = Promise.withResolvers<void>();
    const files = createManagedAttachmentFileCoordinator({
      writeBase64: writeAttachmentBase64,
      writeBytes: async (input) => {
        started.resolve();
        await writing.promise;
        return writeAttachmentBytes(input);
      },
      copyFile: copyAttachmentFileToManagedStorage,
      readBase64: readManagedFileBase64,
      deleteFile: deleteManagedAttachmentFile,
      garbageCollect: garbageCollectManagedAttachmentFiles,
    });
    const first = owner();
    const creation = files.writeBytes({ attachmentId: "creating", bytes: [3] }, first);
    await started.promise;
    const collect = files.garbageCollect({ referencedIds: [] }, first);
    writing.resolve();
    const attachment = await creation;
    expect(await collect).toBe(0);
    await expect(readFile(attachment.path)).resolves.toEqual(Buffer.from([3]));
    expect(await files.garbageCollect({ referencedIds: [] }, first)).toBe(1);
  });

  it("retains a closing window's reported and not-yet-reported attachments until restart", async () => {
    await useTempPaseoHome();
    const files = createManagedAttachmentFileCoordinator();
    const first = owner();
    const second = owner();
    const saved = await files.writeBase64({ attachmentId: "saved", base64: "YQ==" }, first);
    await files.garbageCollect({ referencedIds: ["saved"] }, first);
    const fresh = await files.writeBytes({ attachmentId: "fresh", bytes: [4] }, first);
    first.close();

    expect(await files.garbageCollect({ referencedIds: [] }, second)).toBe(0);
    await expect(readFile(saved.path, "utf8")).resolves.toBe("a");
    await expect(readFile(fresh.path)).resolves.toEqual(Buffer.from([4]));
    // A new main-process lifetime can collect after restored drafts report their references.
    const restarted = createManagedAttachmentFileCoordinator();
    expect(await restarted.garbageCollect({ referencedIds: [] }, second)).toBe(2);
  });

  it("retains writes that complete after the creating window closes", async () => {
    await useTempPaseoHome();
    const files = createManagedAttachmentFileCoordinator();
    const first = owner();
    const creation = files.writeBytes({ attachmentId: "closing", bytes: [5] }, first);
    first.close();
    const attachment = await creation;
    expect(await files.garbageCollect({ referencedIds: [] }, owner())).toBe(0);
    await expect(readFile(attachment.path)).resolves.toEqual(Buffer.from([5]));
  });

  it("uses fresh window references when collection was already scanning files", async () => {
    await useTempPaseoHome();
    const attachment = await writeAttachmentBytes({ attachmentId: "restored", bytes: [8] });
    const scanning = Promise.withResolvers<void>();
    const resume = Promise.withResolvers<void>();
    let pause = true;
    const files = createManagedAttachmentFileCoordinator({
      writeBase64: writeAttachmentBase64,
      writeBytes: writeAttachmentBytes,
      copyFile: copyAttachmentFileToManagedStorage,
      readBase64: readManagedFileBase64,
      deleteFile: deleteManagedAttachmentFile,
      garbageCollect: async (input, isReferenced) => {
        if (pause) {
          pause = false;
          scanning.resolve();
          await resume.promise;
        }
        return garbageCollectManagedAttachmentFiles(input, isReferenced);
      },
    });
    const collect = files.garbageCollect({ referencedIds: [] }, owner());
    await scanning.promise;
    const report = files.garbageCollect({ referencedIds: ["restored"] }, owner());
    resume.resolve();
    expect(await collect).toBe(0);
    expect(await report).toBe(0);
    await expect(readFile(attachment.path)).resolves.toEqual(Buffer.from([8]));
  });

  it("defers explicit deletion while another window references the same attachment", async () => {
    await useTempPaseoHome();
    const files = createManagedAttachmentFileCoordinator();
    const first = owner();
    const second = owner();
    const attachment = await files.writeBytes({ attachmentId: "shared", bytes: [6] }, first);
    await files.garbageCollect({ referencedIds: ["shared"] }, first);
    await files.garbageCollect({ referencedIds: ["shared"] }, second);
    expect(await files.deleteFile({ path: attachment.path }, first)).toBe(true);
    await expect(readFile(attachment.path)).resolves.toEqual(Buffer.from([6]));
    expect(await files.garbageCollect({ referencedIds: [] }, second)).toBe(1);
  });
});
