// Use OMP's public extension API so its journal owns Goal persistence even before
// the first assistant message. Older OMP versions can lack this API.
export default function (pi) {
  const persist = async (_event, ctx) => {
    await ctx.sessionManager.ensureOnDisk?.();
  };
  pi.on("session_start", persist);
  pi.on("session_switch", persist);
}
