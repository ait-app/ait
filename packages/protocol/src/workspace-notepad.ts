import { z } from "zod";

/** Largest notepad text the daemon stores, in UTF-8 bytes. */
export const WORKSPACE_NOTEPAD_MAX_BYTES = 262_144;

/** Who last wrote the notepad; `title` is the agent's title when it wrote. */
export const WorkspaceNotepadAuthorSchema = z.object({
  // A kind added by a later host must not make the notepad unreadable.
  kind: z.union([z.enum(["agent", "bot", "user", "external"]), z.string()]),
  agentId: z.string().nullish(),
  title: z.string().nullish(),
});

export const WorkspaceNotepadGetRequestSchema = z.object({
  type: z.literal("workspace.notepad.get.request"),
  requestId: z.string(),
  workspaceId: z.string().min(1),
});

export const WorkspaceNotepadWriteRequestSchema = z.object({
  type: z.literal("workspace.notepad.write.request"),
  requestId: z.string(),
  workspaceId: z.string().min(1),
  text: z.string(),
  // Defaults to `replace`, which must carry the revision last read (0 before the first write).
  mode: z.enum(["replace", "append"]).optional(),
  expected: z.number().int().nonnegative().optional(),
});

const revision = z.number().int().nonnegative();

/** A notepad nobody has written is revision 0 with empty text and no author. */
export const WorkspaceNotepadSchema = z.object({
  workspaceId: z.string(),
  revision,
  text: z.string(),
  updatedBy: WorkspaceNotepadAuthorSchema.nullish(),
  updatedAt: z.string().nullish(),
});

/**
 * `revision` is the new revision when accepted. Otherwise it is the current one: it differs from
 * `expected` when someone else wrote first, and `error` says why the write was refused.
 */
export const WorkspaceNotepadWriteResultSchema = z.object({
  accepted: z.boolean(),
  workspaceId: z.string(),
  revision,
  updatedBy: WorkspaceNotepadAuthorSchema.nullish(),
  updatedAt: z.string().nullish(),
  error: z.string().nullish(),
});

export const WorkspaceNotepadGetResponseSchema = z.object({
  type: z.literal("workspace.notepad.get.response"),
  payload: WorkspaceNotepadSchema.extend({ requestId: z.string() }),
});

export const WorkspaceNotepadWriteResponseSchema = z.object({
  type: z.literal("workspace.notepad.write.response"),
  payload: WorkspaceNotepadWriteResultSchema.extend({ requestId: z.string() }),
});

export const WORKSPACE_NOTEPAD_METHODS = [
  WorkspaceNotepadGetRequestSchema.shape.type.value,
  WorkspaceNotepadWriteRequestSchema.shape.type.value,
] as const;

export type WorkspaceNotepadAuthor = z.infer<typeof WorkspaceNotepadAuthorSchema>;
export type WorkspaceNotepad = z.infer<typeof WorkspaceNotepadSchema>;
export type WorkspaceNotepadWriteResult = z.infer<typeof WorkspaceNotepadWriteResultSchema>;
