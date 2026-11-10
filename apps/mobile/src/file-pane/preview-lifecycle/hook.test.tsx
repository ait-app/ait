/**
 * @vitest-environment jsdom
 */
import type { FileReadResult } from "@ait/client/internal/daemon-client";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { collectRetainedAttachmentIds } from "@/attachments/gc-retention";
import type { LiveFileSnapshot } from "../live-file/model";
import { useFilePreview } from "./hook";
import { filePreviewFromLifecycle, type FilePreviewLifecycleSnapshot } from "./model";

vi.mock("@/attachments/service", () => ({
  persistAttachmentFromBytes: vi.fn(async (input) => ({
    id: input.id,
    mimeType: input.mimeType,
    fileName: input.fileName,
    storageType: "desktop-file",
    storageKey: `/preview/${input.id}`,
    createdAt: 0,
  })),
}));

function binaryFile(path: string): FileReadResult {
  return {
    bytes: new TextEncoder().encode("archive"),
    mime: "application/zip",
    size: 7,
    path,
    kind: "binary",
    modifiedAt: "2026-08-20T00:00:00.000Z",
    revision: path,
  };
}

function completed(file: FileReadResult): LiveFileSnapshot {
  return {
    observation: {
      status: "ready",
      file,
      version: {
        status: "ready",
        cwd: "/workspace",
        path: file.path,
        size: file.size,
        modifiedAt: file.modifiedAt,
        revision: file.revision,
      },
    },
    read: { status: "idle" },
  };
}

function props(path: string) {
  return { targetKey: `/workspace:${path}`, liveFileSnapshot: completed(binaryFile(path)) };
}

function attachmentId(snapshot: FilePreviewLifecycleSnapshot): string | undefined {
  return filePreviewFromLifecycle(snapshot)?.imageAttachment?.id;
}

describe("useFilePreview attachment retention", () => {
  afterEach(() => {
    expect(collectRetainedAttachmentIds().size).toBe(0);
  });

  it("retains the previewed attachment and releases it on target change and unmount", async () => {
    const { result, rerender, unmount } = renderHook((input) => useFilePreview(input), {
      initialProps: props("first.zip"),
    });
    await waitFor(() => expect(result.current.status).toBe("ready"));
    const firstId = attachmentId(result.current);
    expect(firstId).toBeDefined();
    expect(collectRetainedAttachmentIds()).toEqual(new Set([firstId]));

    rerender(props("second.zip"));
    await waitFor(() => {
      expect(result.current.status).toBe("ready");
      expect(attachmentId(result.current)).not.toBe(firstId);
    });
    expect(collectRetainedAttachmentIds()).toEqual(new Set([attachmentId(result.current)]));

    unmount();
    expect(collectRetainedAttachmentIds().size).toBe(0);
  });
});
