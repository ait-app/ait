export interface OpenIntentConsumption {
  consumedKey: string | null;
  shouldOpen: boolean;
}

/**
 * Decides whether an `?open=` intent still needs handling. The route effect can re-run with the
 * same value before the param clears, so a key is consumed once until the param goes away.
 */
export function resolveOpenIntentConsumption(
  consumedKey: string | null,
  openKey: string | null,
): OpenIntentConsumption {
  if (openKey === null) {
    return { consumedKey: null, shouldOpen: false };
  }
  return { consumedKey: openKey, shouldOpen: consumedKey !== openKey };
}
