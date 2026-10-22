/** A center rejection, distinct from loss of the logical link or an unknown call outcome. */
export class HubCallError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "HubCallError";
  }
}

/** Bounded pending results; transport ACKs deliberately do not complete these calls. */
export class HubCalls {
  private readonly pending = new Map<
    string,
    {
      finish(value: unknown, error?: Error): void;
      bytes: number;
    }
  >();
  private bytes = 0;

  request(id: string, payload: Uint8Array, send: () => void): Promise<unknown> {
    if (this.pending.has(id))
      return Promise.reject(new HubCallError("conflict", "Duplicate center call identity."));
    if (this.pending.size >= 16 || this.bytes + payload.byteLength > 2 * 1024 * 1024)
      return Promise.reject(new HubCallError("capacity", "Too many pending center calls."));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () =>
          finish(
            undefined,
            new HubCallError("timeout", "Center call timed out; its outcome is unknown."),
          ),
        30_000,
      );
      const finish = (value: unknown, error?: Error) => {
        clearTimeout(timer);
        if (!this.pending.delete(id)) return;
        this.bytes -= payload.byteLength;
        if (error) reject(error);
        else resolve(value);
      };
      this.pending.set(id, { finish, bytes: payload.byteLength });
      this.bytes += payload.byteLength;
      try {
        send();
      } catch (error) {
        finish(undefined, error instanceof Error ? error : new Error("Center call failed."));
      }
    });
  }

  reply(id: string, payload: Uint8Array): void {
    const pending = this.pending.get(id);
    if (!pending) return;
    try {
      const value = JSON.parse(new TextDecoder().decode(payload));
      if (
        value?.error &&
        typeof value.error.code === "string" &&
        typeof value.error.message === "string"
      )
        pending.finish(undefined, new HubCallError(value.error.code, value.error.message));
      else if (value && Object.hasOwn(value, "result")) pending.finish(value.result);
      else throw new Error("Invalid center reply.");
    } catch {
      pending.finish(undefined, new HubCallError("invalid_reply", "Invalid center reply."));
    }
  }

  reset(reason: string): void {
    for (const pending of this.pending.values())
      pending.finish(undefined, new HubCallError("link_reset", `Center link reset (${reason}).`));
  }
}
