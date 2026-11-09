import * as Linking from "expo-linking";
import { getDesktopHost } from "@/desktop/host";
import { isWeb } from "@/constants/platform";

import { isHttpUrl } from "./http-url";

export async function openExternalUrl(url: string): Promise<void> {
  if (!isHttpUrl(url) && !/^mailto:/i.test(url)) return;
  if (isWeb) {
    const opener = getDesktopHost()?.opener?.openUrl;
    if (typeof opener === "function") {
      await opener(url);
      return;
    }

    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }

  await Linking.openURL(url);
}

/**
 * Opens a Messages compose link. Separate from `openExternalUrl`, whose callers pass links found
 * in agent and terminal output and must not be able to start a text message.
 */
export async function openSmsUrl(url: string): Promise<void> {
  if (!/^sms:/i.test(url)) return;
  await Linking.openURL(url);
}
