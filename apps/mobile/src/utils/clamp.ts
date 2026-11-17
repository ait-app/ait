/** Limits `value` to [min, max]; when min > max the result is `max`. */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
