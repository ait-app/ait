import { z } from "zod";

/** Largest text a notepad write may leave in the file, in UTF-8 bytes. */
export const WORKSPACE_NOTEPAD_MAX_BYTES = 262_144;

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
  // Defaults to `replace`, which must carry the revision last read unless the notepad is empty.
  mode: z.enum(["replace", "append"]).optional(),
  expected: z.string().min(1).optional(),
});

/**
 * The notepad is a Markdown file the host keeps outside the workspace's checkout. Clients open
 * it with the file methods, using `directory` as `cwd`; `path` is the file itself. `revision`
 * differs whenever the text does.
 */
export const WorkspaceNotepadSchema = z.object({
  workspaceId: z.string(),
  path: z.string().min(1),
  directory: z.string().min(1),
  revision: z.string(),
  text: z.string(),
  updatedAt: z.string().nullish(),
});

/**
 * `revision` is the new revision when accepted. Otherwise it is the current one: it differs from
 * `expected` when someone else wrote first, and `error` says why the write was refused.
 */
export const WorkspaceNotepadWriteResultSchema = z.object({
  accepted: z.boolean(),
  workspaceId: z.string(),
  path: z.string().nullish(),
  revision: z.string(),
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

export type WorkspaceNotepad = z.infer<typeof WorkspaceNotepadSchema>;
export type WorkspaceNotepadWriteResult = z.infer<typeof WorkspaceNotepadWriteResultSchema>;
