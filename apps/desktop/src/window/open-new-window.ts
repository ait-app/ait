import { Buffer } from "node:buffer";
import { z } from "zod";

const WindowTargetIdSchema = z
  .string()
  .trim()
  .min(1)
  .refine((value) => value !== "." && value !== "..");

const NewWindowOptionsSchema = z
  .strictObject({
    pendingOpenProjectPath: z.string().nullable().optional(),
    workspace: z
      .strictObject({
        serverId: WindowTargetIdSchema,
        workspaceId: WindowTargetIdSchema,
      })
      .optional(),
  })
  .refine((value) => !value.workspace || !value.pendingOpenProjectPath?.trim(), {
    message: "A new window can open a workspace or a project, not both",
  });

export type NewWindowOptions = z.input<typeof NewWindowOptionsSchema>;

export function resolveNewWindowOptions(input: unknown): {
  initialRoute: string | null;
  pendingProjectPath: string | null;
} {
  const options = NewWindowOptionsSchema.parse(input ?? {});
  if (!options.workspace) {
    return { initialRoute: null, pendingProjectPath: options.pendingOpenProjectPath ?? null };
  }

  const { serverId, workspaceId } = options.workspace;
  // Keep the workspace segment compatible with the renderer's buildHostWorkspaceRoute.
  const segment = /^[A-Za-z0-9._~-]+$/.test(workspaceId)
    ? workspaceId
    : `b64_${Buffer.from(workspaceId, "utf8").toString("base64url")}`;
  return {
    initialRoute: `/h/${encodeURIComponent(serverId)}/workspace/${encodeURIComponent(segment)}`,
    pendingProjectPath: null,
  };
}
