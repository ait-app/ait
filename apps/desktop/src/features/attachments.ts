import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { resolveDesktopDaemonHome } from "../daemon/rust-daemon.js";

const ATTACHMENTS_DIRNAME = "desktop-attachments";
const ATTACHMENT_ID_PATTERN = /^[A-Za-z0-9_-]+$/;
const EXTENSION_PATTERN = /^\.[A-Za-z0-9]{1,16}$/;

interface AttachmentFileResult {
  path: string;
  byteSize: number;
}

function attachmentsDirPath(): string {
  return path.join(resolveDesktopDaemonHome(process.env), ATTACHMENTS_DIRNAME);
}

async function ensureAttachmentsDir(): Promise<string> {
  const dirPath = attachmentsDirPath();
  await mkdir(dirPath, { recursive: true });
  return dirPath;
}

function normalizeAttachmentId(value: unknown): string {
  if (typeof value !== "string") {
    throw new Error("Attachment id is required.");
  }
  const normalized = value.trim();
  if (!ATTACHMENT_ID_PATTERN.test(normalized)) {
    throw new Error(`Invalid attachment id: ${value}`);
  }
  return normalized;
}

function normalizeExtension(value: unknown): string {
  if (typeof value !== "string") {
    return ".bin";
  }
  const normalized = value.trim().toLowerCase();
  const extension = normalized.startsWith(".") ? normalized : `.${normalized}`;
  // The extension only labels the managed copy, so unusual names fall back instead of failing.
  return EXTENSION_PATTERN.test(extension) ? extension : ".bin";
}

async function buildManagedAttachmentPath(input: {
  attachmentId: unknown;
  extension: unknown;
}): Promise<string> {
  const dirPath = await ensureAttachmentsDir();
  const attachmentId = normalizeAttachmentId(input.attachmentId);
  const extension = normalizeExtension(input.extension);
  return path.join(dirPath, `${attachmentId}${extension}`);
}

function resolveManagedAttachmentPath(inputPath: unknown): string {
  if (typeof inputPath !== "string" || inputPath.trim().length === 0) {
    throw new Error("Attachment path is required.");
  }
  const resolvedDir = `${path.resolve(attachmentsDirPath())}${path.sep}`;
  const resolvedPath = path.resolve(inputPath.trim());
  if (!resolvedPath.startsWith(resolvedDir)) {
    throw new Error("Attachment path must stay within desktop-managed storage.");
  }
  return resolvedPath;
}

export async function writeAttachmentBase64(input: {
  attachmentId?: unknown;
  base64?: unknown;
  extension?: unknown;
}): Promise<AttachmentFileResult> {
  const base64 = typeof input.base64 === "string" ? input.base64.trim() : "";
  if (base64.length === 0) {
    throw new Error("Attachment base64 payload is required.");
  }

  const targetPath = await buildManagedAttachmentPath({
    attachmentId: input.attachmentId,
    extension: input.extension,
  });
  await writeFile(targetPath, Buffer.from(base64, "base64"));
  const fileInfo = await stat(targetPath);
  return {
    path: targetPath,
    byteSize: fileInfo.size,
  };
}

function normalizeBytes(value: unknown): Uint8Array {
  if (value instanceof Uint8Array) {
    return value;
  }
  if (value instanceof ArrayBuffer) {
    return new Uint8Array(value);
  }
  if (Array.isArray(value)) {
    return Uint8Array.from(value);
  }
  throw new Error("Attachment byte payload is required.");
}

export async function writeAttachmentBytes(input: {
  attachmentId?: unknown;
  bytes?: unknown;
  extension?: unknown;
}): Promise<AttachmentFileResult> {
  const bytes = normalizeBytes(input.bytes);
  const targetPath = await buildManagedAttachmentPath({
    attachmentId: input.attachmentId,
    extension: input.extension,
  });
  await writeFile(targetPath, bytes);
  const fileInfo = await stat(targetPath);
  return {
    path: targetPath,
    byteSize: fileInfo.size,
  };
}

export async function copyAttachmentFileToManagedStorage(input: {
  attachmentId?: unknown;
  sourcePath?: unknown;
  extension?: unknown;
}): Promise<AttachmentFileResult> {
  if (typeof input.sourcePath !== "string" || input.sourcePath.trim().length === 0) {
    throw new Error("Attachment source path is required.");
  }

  const sourcePath = path.resolve(input.sourcePath.trim());
  const targetPath = await buildManagedAttachmentPath({
    attachmentId: input.attachmentId,
    extension: input.extension,
  });

  if (sourcePath !== targetPath) {
    await copyFile(sourcePath, targetPath);
  }

  const fileInfo = await stat(targetPath);
  return {
    path: targetPath,
    byteSize: fileInfo.size,
  };
}

export async function readManagedFileBase64(input: { path?: unknown }): Promise<string> {
  const filePath = resolveManagedAttachmentPath(input.path);
  const bytes = await readFile(filePath);
  return bytes.toString("base64");
}

export async function deleteManagedAttachmentFile(input: { path?: unknown }): Promise<boolean> {
  const filePath = resolveManagedAttachmentPath(input.path);
  await rm(filePath, { force: true });
  return true;
}

export async function garbageCollectManagedAttachmentFiles(
  input: { referencedIds?: unknown },
  isReferenced?: (attachmentId: string) => boolean,
): Promise<number> {
  const dirPath = await ensureAttachmentsDir();
  const referencedIds = normalizeReferencedIds(input.referencedIds);

  const entries = await readdir(dirPath, { withFileTypes: true });
  const deleted = await Promise.all(
    entries.map(async (entry) => {
      const id = path.parse(entry.name).name;
      // A window can report fresh references while the directory read is in flight.
      if (!entry.isFile() || referencedIds.has(id) || isReferenced?.(id)) return false;
      await rm(path.join(dirPath, entry.name), { force: true });
      return true;
    }),
  );
  return deleted.filter(Boolean).length;
}

function normalizeReferencedIds(input: unknown): Set<string> {
  return Array.isArray(input)
    ? new Set(
        input
          .filter((value): value is string => typeof value === "string")
          .map((value) => value.trim())
          .filter((value) => ATTACHMENT_ID_PATTERN.test(value)),
      )
    : new Set<string>();
}

export interface AttachmentOwner {
  once(event: "destroyed", listener: () => void): unknown;
  isDestroyed(): boolean;
}

interface OwnerReferences {
  referenced: Set<string>;
  created: Set<string>;
  creating: Map<string, number>;
  closed: boolean;
}

const attachmentFileOperations = {
  writeBase64: writeAttachmentBase64,
  writeBytes: writeAttachmentBytes,
  copyFile: copyAttachmentFileToManagedStorage,
  readBase64: readManagedFileBase64,
  deleteFile: deleteManagedAttachmentFile,
  garbageCollect: garbageCollectManagedAttachmentFiles,
};

/** Keep a renderer's partial draft view from deleting another window's attachments. */
export function createManagedAttachmentFileCoordinator(operations = attachmentFileOperations) {
  const owners = new Map<AttachmentOwner, OwnerReferences>();
  const closedWindowReferences = new Set<string>();
  const fallbackOwner: AttachmentOwner = { once: () => {}, isDestroyed: () => false };
  let tail = Promise.resolve();

  const run = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = tail.then(operation);
    tail = result.then(
      () => {},
      () => {},
    );
    return result;
  };

  const referencesFor = (owner = fallbackOwner): OwnerReferences => {
    if (owner.isDestroyed()) throw new Error("Attachment window has been destroyed");
    const existing = owners.get(owner);
    if (existing) return existing;
    const references: OwnerReferences = {
      referenced: new Set(),
      created: new Set(),
      creating: new Map(),
      closed: false,
    };
    owners.set(owner, references);
    owner.once("destroyed", () => {
      // Another window may still have an older draft snapshot. Preserve the closed window's
      // persisted and not-yet-reported attachments until restart, when drafts hydrate together.
      references.closed = true;
      for (const id of [
        ...references.referenced,
        ...references.created,
        ...references.creating.keys(),
      ]) {
        closedWindowReferences.add(id);
      }
      owners.delete(owner);
    });
    return references;
  };

  const isReferenced = (id: string): boolean => {
    if (closedWindowReferences.has(id)) return true;
    for (const references of owners.values()) {
      if (
        references.referenced.has(id) ||
        references.created.has(id) ||
        references.creating.has(id)
      )
        return true;
    }
    return false;
  };

  const create = (
    attachmentId: unknown,
    owner: AttachmentOwner | undefined,
    operation: () => Promise<AttachmentFileResult>,
  ): Promise<AttachmentFileResult> => {
    const id = normalizeAttachmentId(attachmentId);
    const references = referencesFor(owner);
    references.creating.set(id, (references.creating.get(id) ?? 0) + 1);
    return run(async () => {
      try {
        const result = await operation();
        if (references.closed) closedWindowReferences.add(id);
        else references.created.add(id);
        return result;
      } finally {
        const remaining = (references.creating.get(id) ?? 1) - 1;
        if (remaining > 0) references.creating.set(id, remaining);
        else references.creating.delete(id);
      }
    });
  };

  return {
    writeBase64: (input: Parameters<typeof writeAttachmentBase64>[0], owner?: AttachmentOwner) =>
      create(input.attachmentId, owner, () => operations.writeBase64(input)),
    writeBytes: (input: Parameters<typeof writeAttachmentBytes>[0], owner?: AttachmentOwner) =>
      create(input.attachmentId, owner, () => operations.writeBytes(input)),
    copyFile: (
      input: Parameters<typeof copyAttachmentFileToManagedStorage>[0],
      owner?: AttachmentOwner,
    ) => create(input.attachmentId, owner, () => operations.copyFile(input)),
    readBase64: (input: Parameters<typeof readManagedFileBase64>[0], owner?: AttachmentOwner) => {
      const id = normalizeAttachmentId(path.parse(resolveManagedAttachmentPath(input.path)).name);
      referencesFor(owner).created.add(id);
      return run(() => operations.readBase64(input));
    },
    deleteFile: (
      input: Parameters<typeof deleteManagedAttachmentFile>[0],
      owner?: AttachmentOwner,
    ) => {
      const id = normalizeAttachmentId(path.parse(resolveManagedAttachmentPath(input.path)).name);
      const references = referencesFor(owner);
      references.referenced.delete(id);
      references.created.delete(id);
      return run(() => (isReferenced(id) ? Promise.resolve(true) : operations.deleteFile(input)));
    },
    garbageCollect: (
      input: Parameters<typeof garbageCollectManagedAttachmentFiles>[0],
      owner?: AttachmentOwner,
    ) => {
      const references = referencesFor(owner);
      references.referenced = normalizeReferencedIds(input.referencedIds);
      references.created.clear();
      return run(() => operations.garbageCollect({ referencedIds: [] }, isReferenced));
    },
  };
}
