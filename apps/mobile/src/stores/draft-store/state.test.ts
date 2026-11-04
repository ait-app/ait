import { describe, expect, it } from "vitest";
import type { AttachmentMetadata, ComposerAttachment } from "@/attachments/types";
import {
  applyClearDraftRecord,
  CanonicalDraftInputSchema,
  collectQueuedMessageAttachmentIds,
  editDraftRecordText,
  pruneFinalizedDraftRecords,
  toDraftInputIfReady,
  type DraftRecord,
} from "./state";

describe("draft-store lifecycle", () => {
  it("edits text without invalidating attachment subscribers", () => {
    const draft: DraftRecord = {
      input: {
        text: "hello",
        attachments: [
          { kind: "workspace_file", path: "README.md", selection: { kind: "whole_file" } },
        ],
      },
      lifecycle: "active",
      updatedAt: 1,
      version: 1,
    };
    const edited = editDraftRecordText(draft, "hello\n", 2);
    expect(edited.input.text).toBe("hello\n");
    expect(edited.input.attachments).toBe(draft.input.attachments);
    expect(edited.version).toBe(2);
    expect(editDraftRecordText(edited, "hello\n", 3)).toBe(edited);
    expect(editDraftRecordText(edited, "", 3).lifecycle).toBe("active");
    expect(editDraftRecordText(undefined, "", 3).lifecycle).toBe("abandoned");
  });
  it("prunes finalized tombstones after TTL", () => {
    const nowMs = 1_000_000;
    const drafts = {
      oldSent: {
        input: { text: "", attachments: [] },
        lifecycle: "sent" as const,
        updatedAt: 0,
        version: 2,
      },
      recentAbandoned: {
        input: { text: "", attachments: [] },
        lifecycle: "abandoned" as const,
        updatedAt: nowMs + 2 * 60 * 1000,
        version: 2,
      },
      active: {
        input: { text: "a", attachments: [] },
        lifecycle: "active" as const,
        updatedAt: 0,
        version: 1,
      },
    };

    const pruned = pruneFinalizedDraftRecords({
      drafts,
      nowMs: nowMs + 6 * 60 * 1000,
    });

    expect(pruned.oldSent).toBeUndefined();
    expect(pruned.recentAbandoned).toBeDefined();
    expect(pruned.active).toBeDefined();
  });

  it("normalizes clear-with-lifecycle into a tombstone without attachments", () => {
    const cleared = applyClearDraftRecord({
      record: {
        input: {
          text: "hello",
          attachments: [
            {
              kind: "image",
              metadata: {
                id: "att-1",
                mimeType: "image/jpeg",
                storageType: "web-indexeddb",
                storageKey: "att-1",
                createdAt: 1,
              },
            },
          ],
        },
        lifecycle: "active",
        updatedAt: 1,
        version: 1,
      },
      lifecycle: "sent",
      nowMs: 2,
    });

    expect(cleared).toEqual({
      input: { text: "", attachments: [] },
      lifecycle: "sent",
      updatedAt: 2,
      version: 2,
    });
  });
});

describe("draft-store normalization", () => {
  it("preserves uploaded file attachments when hydrating a draft", () => {
    const attachment = {
      kind: "file" as const,
      attachment: {
        type: "uploaded_file" as const,
        id: "file-1",
        fileName: "context.json",
        mimeType: "application/json",
        size: 42,
        path: "/tmp/context.json",
      },
    };

    expect(
      toDraftInputIfReady({
        input: { text: "Review this", attachments: [attachment] },
        lifecycle: "active",
        updatedAt: 1,
        version: 1,
      }),
    ).toEqual({ text: "Review this", attachments: [attachment] });
  });

  it("preserves workspace file selections when hydrating a draft", () => {
    const attachment = {
      kind: "workspace_file" as const,
      path: "src/app.ts",
      selection: { kind: "line_range" as const, startLine: 12, endLine: 24 },
    };

    expect(
      toDraftInputIfReady({
        input: { text: "Review this", attachments: [attachment] },
        lifecycle: "active",
        updatedAt: 1,
        version: 1,
      }),
    ).toEqual({ text: "Review this", attachments: [attachment] });
  });

  it("preserves New Workspace picker ownership when hydrating a draft", () => {
    const pickerAttachment = {
      kind: "github_pr" as const,
      owner: "new-workspace-picker" as const,
      item: {
        kind: "change_request" as const,
        number: 202,
        title: "Persist picker ownership",
        url: "https://example.com/pull/202",
        state: "open" as const,
        body: null,
        labels: [],
        baseRefName: "main",
        headRefName: "feature/picker-ownership",
      },
    };

    expect(
      toDraftInputIfReady({
        input: { text: "Keep this prompt", attachments: [pickerAttachment] },
        lifecycle: "active",
        updatedAt: 1,
        version: 1,
      }),
    ).toEqual({
      text: "Keep this prompt",
      attachments: [pickerAttachment],
    });
  });
});

it("drops removed plugin attachments without losing the draft text or ordinary attachments", () => {
  const parsed = CanonicalDraftInputSchema.parse({
    text: "keep my draft",
    attachments: [
      { kind: "plugin_resource", pluginId: "retired", sourceId: "items", item: { id: "one" } },
      { kind: "workspace_file", path: "README.md", selection: { kind: "whole_file" } },
    ],
  });
  expect(parsed.text).toBe("keep my draft");
  expect(parsed.attachments).toEqual([
    { kind: "workspace_file", path: "README.md", selection: { kind: "whole_file" } },
  ]);
});

it("keeps queued images and browser-element screenshots referenced", () => {
  const metadata = (id: string): AttachmentMetadata => ({
    id,
    mimeType: "image/png",
    storageType: "web-indexeddb",
    storageKey: id,
    createdAt: 1,
  });
  const browserElement: ComposerAttachment = {
    kind: "browser_element",
    attachment: {
      url: "http://localhost/",
      selector: "#app",
      tag: "div",
      text: "",
      outerHTML: "<div></div>",
      computedStyles: {},
      boundingRect: { x: 0, y: 0, width: 1, height: 1 },
      reactSource: null,
      parentChain: [],
      children: [],
      screenshot: metadata("screenshot"),
      formatted: "<div></div>",
    },
  };
  const referencedIds = new Set<string>();
  collectQueuedMessageAttachmentIds(
    new Map([
      [
        "agent-1",
        [
          {
            attachments: [{ kind: "image", metadata: metadata("image") }, browserElement],
          },
        ],
      ],
    ]),
    referencedIds,
  );
  expect([...referencedIds].sort()).toEqual(["image", "screenshot"]);
});
