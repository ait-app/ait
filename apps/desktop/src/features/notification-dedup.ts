const delivered = new Map<string, number>();

/** The same attention may reach several renderers through Hub and direct Host sessions. */
export function claimDesktopNotification(data: Record<string, unknown> | undefined): boolean {
  const attentionKey =
    typeof data?.attentionKey === "string" && data.attentionKey.length <= 4096
      ? data.attentionKey
      : null;
  const key =
    attentionKey ??
    (typeof data?.eventId === "string" && typeof data.hostId === "string"
      ? JSON.stringify([data.hostId, data.eventId])
      : null);
  if (!key) return true;
  const now = Date.now();
  if ((delivered.get(key) ?? 0) > now) return false;
  delivered.delete(key);
  delivered.set(key, now + (data?.terminalId ? 5000 : 24 * 60 * 60_000));
  while (delivered.size > 4096) delivered.delete(delivered.keys().next().value!);
  return true;
}
