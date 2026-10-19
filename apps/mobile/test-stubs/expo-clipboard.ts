// Keep browser component tests inside their own clipboard boundary.
export async function setStringAsync(_text: string): Promise<boolean> {
  return true;
}
export async function getStringAsync(): Promise<string> {
  return "";
}
export function setString(_text: string): void {}
export const StringFormat = { PLAIN_TEXT: "plainText", HTML: "html" } as const;
