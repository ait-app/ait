import { closeSync, fstatSync, openSync, readSync } from "node:fs";

interface TailFileOptions {
  throwOnReadError?: boolean;
}

// Diagnostics show at most a few hundred lines, so long-lived logs are read from the end only.
const TAIL_READ_LIMIT_BYTES = 512 * 1024;

export function tailFile(filePath: string, lines = 50, options: TailFileOptions = {}): string {
  try {
    return readTailLines(filePath, lines);
  } catch (error) {
    if (options.throwOnReadError && !isMissingFileError(error)) {
      throw error;
    }
    return "";
  }
}

function readTailLines(filePath: string, lines: number): string {
  const descriptor = openSync(filePath, "r");
  try {
    const { size } = fstatSync(descriptor);
    const length = Math.min(size, TAIL_READ_LIMIT_BYTES);
    const buffer = Buffer.alloc(length);
    const bytesRead = readSync(descriptor, buffer, 0, length, size - length);
    const segments = buffer.toString("utf-8", 0, bytesRead).split("\n");
    // A truncated read starts mid-line; drop that partial first line.
    if (length < size) segments.shift();
    return segments.filter(Boolean).slice(-lines).join("\n");
  } finally {
    closeSync(descriptor);
  }
}

function isMissingFileError(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT";
}
