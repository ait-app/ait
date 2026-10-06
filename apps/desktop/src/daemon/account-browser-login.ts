import { createHash, randomBytes } from "node:crypto";
import { createServer } from "node:http";
import {
  parseAccountCallback,
  type AccountBrowserLogin,
} from "@ait/client/internal/account-browser-login";

/** The system browser returns to a temporary listener bound only to loopback. */
export function desktopBrowserLogin(
  openExternal: (url: string) => Promise<unknown>,
): AccountBrowserLogin {
  return {
    randomSecret: () => randomBytes(32).toString("hex"),
    challenge: async (verifier) => createHash("sha256").update(verifier).digest("base64url"),
    open: (authorizationUrl, state, signal) =>
      new Promise((resolve, reject) => {
        let redirectUri = "";
        let settled = false;
        const finish = (error?: Error, url?: string) => {
          if (settled) return;
          settled = true;
          signal.removeEventListener("abort", cancel);
          server.close();
          server.closeAllConnections();
          if (error) reject(error);
          else resolve({ url: url!, redirectUri });
        };
        const cancel = () => finish(new Error("Sign-in cancelled."));
        const server = createServer((request, response) => {
          const url = `http://127.0.0.1:${(server.address() as { port: number }).port}${request.url ?? ""}`;
          const valid = request.method === "GET" && parseAccountCallback(url, redirectUri, state);
          response.writeHead(valid ? 200 : 400, {
            "Content-Type": "text/plain; charset=utf-8",
            "Cache-Control": "no-store",
            "Referrer-Policy": "no-referrer",
            "Content-Security-Policy": "default-src 'none'",
          });
          response.end(
            valid
              ? "Return to AIT to continue. You can close this browser tab."
              : "Invalid sign-in callback.",
            () => {
              if (valid) finish(undefined, url);
            },
          );
        });
        server.on("error", () => finish(new Error("Could not start the local sign-in callback.")));
        signal.addEventListener("abort", cancel, { once: true });
        if (signal.aborted) {
          cancel();
          return;
        }
        server.listen(0, "127.0.0.1", () => {
          if (settled) {
            server.close();
            return;
          }
          redirectUri = `http://127.0.0.1:${(server.address() as { port: number }).port}/auth/callback`;
          void openExternal(authorizationUrl(redirectUri)).catch(() =>
            finish(new Error("Could not open the system browser.")),
          );
        });
        server.unref();
      }),
  };
}
