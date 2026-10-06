import { Linking } from "react-native";
import * as Crypto from "expo-crypto";
import {
  parseAccountCallback,
  type AccountBrowserLogin,
} from "@ait/client/internal/account-browser-login";

export const androidBrowserLogin: AccountBrowserLogin = {
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
