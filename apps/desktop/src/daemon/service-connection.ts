import { ServerListenSchema } from "@ait/protocol/server-listen";
import { constants } from "node:fs";
import { open } from "node:fs/promises";
import path from "node:path";
import { z } from "zod";

export type DaemonMode = "auto" | "desktop" | "external";

const ConnectionSchema = z.object({
  version: z.literal(1),
  managementMode: z.literal("external"),
  serverId: z.uuid(),
  instanceId: z.uuid(),
  pid: z.number().int().positive(),
  startedAt: z.string().datetime({ offset: true }),
  listen: ServerListenSchema,
  token: z
    .string()
    .min(32)
    .max(256)
    .regex(/^[!-~]+$/),
});
export type ServiceConnection = z.output<typeof ConnectionSchema>;

/** Read without consuming the record. An invalid/stale service never authorizes child spawn. */
export async function readServiceConnection(home: string): Promise<ServiceConnection | null> {
  let file;
  try {
    file = await open(
      path.join(home, "service-connection.json"),
      constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0),
    );
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw new Error("Cannot open the private service connection record.");
  }
  try {
    const stat = await file.stat();
    if (
      !stat.isFile() ||
      stat.size > 4096 ||
      (process.platform !== "win32" &&
        ((stat.mode & 0o077) !== 0 || stat.uid !== process.getuid?.()))
    )
      throw new Error("Unsafe service connection record permissions or ownership.");
    const parsed = ConnectionSchema.safeParse(JSON.parse(await file.readFile("utf8")));
    if (!parsed.success) throw new Error("Invalid service connection record.");
    return parsed.data;
  } catch (error) {
    if (error instanceof SyntaxError) throw new Error("Invalid service connection record.");
    throw error;
  } finally {
    await file.close();
  }
}
