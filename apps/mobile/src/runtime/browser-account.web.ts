import {
  AccountSessionManager,
  type AccountSnapshot,
  type SavedAccount,
} from "@ait/client/internal/account-session";
import { acquireBrowserInstallation } from "./browser-installation";
import appPackage from "../../package.json";
import { HubLink } from "@ait/client/internal/hub";
import { defaultWebSocketFactory } from "@ait/client/internal/daemon-client-websocket-transport";

const TOKEN_KEY = "ait.access-token";
const listeners = new Set<(state: AccountSnapshot) => void>();
const transports = new Set<() => void>();
let ready: Promise<AccountSessionManager> | undefined;
let hub: HubLink | null = null;

export function supportsBrowserAccount(): boolean {
  return process.env.EXPO_PUBLIC_AIT_CONSOLE === "1";
}

function read(storage: Storage, key: string): string | null {
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

/** Same-origin console owns sign-in; credentials never travel in a URL or postMessage. */
async function createAccount(): Promise<AccountSessionManager> {
  const center = `${window.location.origin}/api`;
  const lease = await acquireBrowserInstallation();
  try {
    const { installationId } = lease;
    const NODE_KEY = `ait.browser.node-session.${installationId}`;
    const token = read(window.localStorage, TOKEN_KEY);
    class BrowserAccount extends AccountSessionManager {
      override snapshot(): AccountSnapshot {
        return { ...super.snapshot(), center };
      }
      override async loginWithBrowser(value: string): Promise<AccountSnapshot> {
        if (value && value.replace(/\/+$/, "") !== center)
          throw new Error("Use this website's online service to sign in.");
        window.location.assign("/login?client=1");
        return this.snapshot();
      }
      override async logout(): Promise<void> {
        await super.logout();
        hub?.stop();
        if (token) {
          await fetch(`${center}/v1/auth/logout`, {
            method: "POST",
            headers: { Authorization: `Bearer ${token}` },
            credentials: "omit",
            signal: AbortSignal.timeout(10_000),
          }).catch(() => undefined);
        }
      }
    }
    const manager = new BrowserAccount({
      installationId,
      deviceName: "AIT Web",
      platform: "web",
      randomUUID: () => crypto.randomUUID(),
      appVersion: appPackage.version,
      // Same-origin page; a popup opened after the confirmation dialog would be blocked.
      openUrl: async (url) => window.location.assign(url),
      save: async (account) => {
        try {
          if (account?.nodeSessionId) localStorage.setItem(NODE_KEY, account.nodeSessionId);
          if (!account) {
            localStorage.removeItem(NODE_KEY);
            if (read(localStorage, TOKEN_KEY) === token) localStorage.removeItem(TOKEN_KEY);
          }
        } catch {
          // An active in-memory session remains usable when storage becomes unavailable.
        }
      },
      notify: () => {
        if (manager.snapshot().status === "logged_out") hub?.stop();
        else hub?.start();
        for (const listener of listeners) listener(manager.snapshot());
      },
      closeTransports: () => {
        for (const close of Array.from(transports)) close();
      },
    });
    hub = new HubLink({
      ticket: () => manager.openHubTicket(),
      webSocketFactory: defaultWebSocketFactory,
      authentication: "subprotocol",
    });
    manager.attachHub(hub);
    if (token) {
      const response = await fetch(`${center}/v1/me`, {
        headers: { Authorization: `Bearer ${token}` },
        credentials: "omit",
        cache: "no-store",
        signal: AbortSignal.timeout(20_000),
      });
      if (read(localStorage, TOKEN_KEY) !== token)
        throw new Error("The account changed. Reload the page.");
      if (response.status === 401 || response.status === 403) {
        if (read(localStorage, TOKEN_KEY) === token) localStorage.removeItem(TOKEN_KEY);
        localStorage.removeItem(NODE_KEY);
      } else {
        if (!response.ok) throw new Error("Could not reach the account service. Try again.");
        const user = (await response.json()) as {
          email?: string | null;
          phone_number?: string | null;
          display_name?: string | null;
          expires_at?: string | null;
        };
        // The server has already validated the JWT. Read exp only to schedule local expiry.
        const payload = token.split(".")[1];
        let exp: unknown;
        try {
          exp = payload
            ? JSON.parse(atob(payload.replace(/-/g, "+").replace(/_/g, "/"))).exp
            : undefined;
        } catch {
          // Malformed saved data requires a fresh sign-in, never a broken startup loop.
        }
        if (typeof exp !== "number" || !Number.isFinite(exp) || exp * 1000 <= Date.now()) {
          if (read(localStorage, TOKEN_KEY) === token) localStorage.removeItem(TOKEN_KEY);
        } else {
          const saved: SavedAccount = {
            center,
            token,
            expiresAt: exp * 1000,
            name: user.display_name || user.email || user.phone_number || "AIT",
            accountExpiresAt: user.expires_at,
            nodeSessionId: read(localStorage, NODE_KEY) ?? undefined,
          };
          await manager.restore(saved);
        }
      }
    }
    // A different console login/explicit logout invalidates this tab's old authority.
    window.addEventListener("storage", (event) => {
      if (
        (event.key === TOKEN_KEY || event.key === null) &&
        read(localStorage, TOKEN_KEY) !== token
      ) {
        manager.suspend();
        hub?.stop();
        void manager.shutdown().finally(() => window.location.reload());
      }
    });
    window.addEventListener("online", () => manager.refresh());
    window.addEventListener("focus", () => manager.refresh());
    window.addEventListener("pagehide", () => {
      hub?.stop();
      void manager.shutdown();
      lease.release();
    });
    window.addEventListener("pageshow", (event) => {
      if (event.persisted) window.location.reload();
    });
    return manager;
  } catch (error) {
    hub?.stop();
    hub = null;
    lease.release();
    throw error;
  }
}

export function getBrowserHubLink(): HubLink | Promise<HubLink | null> {
  return (
    hub ??
    getBrowserAccount().then(
      () => hub,
      () => null,
    )
  );
}

export function getBrowserAccount(): Promise<AccountSessionManager> {
  ready ??= createAccount().catch((error) => {
    ready = undefined;
    throw error;
  });
  return ready;
}
export function subscribeBrowserAccount(listener: (state: AccountSnapshot) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export function registerBrowserAccountTransport(close: () => void): () => void {
  if (transports.size >= 16) throw new Error("Too many account relay connections.");
  transports.add(close);
  return () => {
    transports.delete(close);
  };
}
