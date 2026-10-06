import { Linking } from "react-native";
import * as Crypto from "expo-crypto";
import * as WebBrowser from "expo-web-browser";
import {
  parseAccountCallback,
  type AccountBrowserLogin,
} from "@ait/client/internal/account-browser-login";

const mobileBrowserCrypto: Pick<AccountBrowserLogin, "randomSecret" | "challenge"> = {
  randomSecret: () =>
    Array.from(Crypto.getRandomValues(new Uint8Array(32)), (value) =>
      value.toString(16).padStart(2, "0"),
    ).join(""),
  challenge: async (verifier) =>
    (
      await Crypto.digestStringAsync(Crypto.CryptoDigestAlgorithm.SHA256, verifier, {
        encoding: Crypto.CryptoEncoding.BASE64,
      })
    )
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, ""),
};

export const androidBrowserLogin: AccountBrowserLogin = {
  ...mobileBrowserCrypto,
  open: (authorizationUrl, state, signal) =>
    new Promise((resolve, reject) => {
      const redirectUri = "ait://auth/callback";
      let settled = false;
      const finish = (error?: Error, url?: string) => {
        if (settled) return;
        settled = true;
        subscription.remove();
        signal.removeEventListener("abort", cancel);
        if (error) reject(error);
        else resolve({ url: url!, redirectUri });
      };
      const cancel = () => finish(new Error("Sign-in cancelled."));
      const subscription = Linking.addEventListener("url", ({ url }) => {
        if (parseAccountCallback(url, redirectUri, state)) finish(undefined, url);
      });
      signal.addEventListener("abort", cancel, { once: true });
      if (signal.aborted) {
        cancel();
        return;
      }
      void Linking.openURL(authorizationUrl(redirectUri)).catch(() =>
        finish(new Error("Could not open the system browser.")),
      );
    }),
};

/** iOS delivers the callback through ASWebAuthenticationSession, not Linking events. */
export const iosBrowserLogin: AccountBrowserLogin = {
  ...mobileBrowserCrypto,
  open: (authorizationUrl, state, signal) =>
    new Promise((resolve, reject) => {
      const redirectUri = "ait://auth/callback";
      if (signal.aborted) {
        reject(new Error("Sign-in cancelled."));
        return;
      }
      let settled = false;
      const finish = (error?: Error, url?: string) => {
        if (settled) return;
        settled = true;
        signal.removeEventListener("abort", cancel);
        if (error) reject(error);
        else resolve({ url: url!, redirectUri });
      };
      const cancel = () => {
        if (settled) return;
        try {
          WebBrowser.dismissAuthSession();
        } catch {
          // Cancellation still invalidates this attempt if the native sheet already closed.
        }
        finish(new Error("Sign-in cancelled."));
      };
      const failed = () => finish(new Error("Could not open the system browser."));
      signal.addEventListener("abort", cancel, { once: true });
      try {
        void WebBrowser.openAuthSessionAsync(authorizationUrl(redirectUri), redirectUri).then(
          (result) => {
            if (result.type === "success") {
              if (parseAccountCallback(result.url, redirectUri, state))
                finish(undefined, result.url);
              else finish(new Error("The sign-in callback is invalid. Start sign-in again."));
            } else if (result.type === "cancel" || result.type === "dismiss") {
              finish(new Error("Sign-in cancelled."));
            } else {
              failed();
            }
          },
          failed,
        );
      } catch {
        failed();
      }
    }),
};
