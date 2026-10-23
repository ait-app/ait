const delivered = new Map<string, number>();

/** Share notification ownership between Hub replay and a live direct/LAN daemon connection. */
export function claimAttentionNotification(key: string, ttlMs = 24 * 60 * 60_000): boolean {
  const now = Date.now();
  const until = delivered.get(key);
  if (until !== undefined && until > now) return false;
  delivered.delete(key);
  delivered.set(key, now + ttlMs);
  while (delivered.size > 4096) delivered.delete(delivered.keys().next().value!);
  return true;
}
