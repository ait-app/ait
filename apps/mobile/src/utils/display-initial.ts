/** The first full character of `value`, uppercased: emoji stay whole, unlike `charAt(0)`. */
export function displayInitial(value: string): string {
  return (Array.from(value)[0] ?? "").toUpperCase();
}
