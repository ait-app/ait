/** Start browser clipboard access immediately inside the user's gesture. */
export async function copyErrorLog(text: string): Promise<void> {
  if (typeof navigator === "undefined" || !navigator.clipboard?.writeText) {
    throw new Error("Clipboard access is unavailable");
  }
  await navigator.clipboard.writeText(text);
}
