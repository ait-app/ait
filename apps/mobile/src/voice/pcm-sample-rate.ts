/** The `rate=` parameter of a PCM mime type ("audio/pcm;rate=24000"), or null when absent or invalid. */
export function parsePcmSampleRate(mimeType: string): number | null {
  const match = /rate=(\d+)/i.exec(mimeType);
  if (!match) {
    return null;
  }
  const rate = Number(match[1]);
  return Number.isFinite(rate) && rate > 0 ? rate : null;
}
