import { copyToClipboard } from "@/utils/copy-to-clipboard";

/** Copy a diagnostic report using the native clipboard implementation. */
export async function copyErrorLog(text: string): Promise<void> {
  await copyToClipboard(text);
}
