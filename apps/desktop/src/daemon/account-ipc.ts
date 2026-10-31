import { displayAppVersion } from "../app-version.js";
import { app, BrowserWindow, safeStorage, shell, type IpcMainInvokeEvent } from "electron";
import { randomUUID } from "node:crypto";
import { WebSocket } from "ws";
import { HubLink } from "@ait/client/internal/hub";
import type { WebSocketLike } from "@ait/client/internal/daemon-client-transport-types";
import { readFileSync } from "node:fs";
import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import {
  AccountSessionManager,
  type AccountSnapshot,
  type SavedAccount,
} from "./account-session.js";
import { AccountTransportManager } from "./account-transport.js";
import { AccountHubBridge } from "./account-hub.js";
import { AccountDownloadManager } from "./account-download.js";
import type { RustDaemonManager } from "./rust-daemon.js";
import { desktopBrowserLogin } from "./account-browser-login.js";
import { AccountBrowserProfiles } from "../features/account-browser-profiles.js";

let account: AccountSessionManager | null = null;
let browserProfiles: AccountBrowserProfiles | null = null;
let accountHub: HubLink | null = null;

export function accountBrowserPartitions(ownerId: number): string[] {
  return browserProfiles?.partitions(ownerId) ?? [];
}

function canRemember(): boolean {
  return (
    safeStorage.isEncryptionAvailable() &&
    (process.platform !== "linux" || safeStorage.getSelectedStorageBackend() !== "basic_text")
  );
}

export async function stopAccountForExit(): Promise<void> {
  accountHub?.stop();
  await account?.shutdown();
}

/** Account commands are admitted only from the application's own top-level windows. */
export function createAccountIpc(
  getRuntime: () => RustDaemonManager,
  isAppWindow: (contents: Electron.WebContents) => boolean,
) {
  const file = path.join(app.getPath("userData"), "account-node.json");
  let installationId: string = randomUUID();
  let saved: SavedAccount | null = null;
  let syncBuiltInDaemon = true;
  try {
    const stored = JSON.parse(readFileSync(file, "utf8")) as {
      installationId?: string;
      account?: string;
      syncBuiltInDaemon?: boolean;
    };
    if (typeof stored.installationId === "string" && /^[0-9a-f-]{36}$/i.test(stored.installationId))
      installationId = stored.installationId;
    if (typeof stored.syncBuiltInDaemon === "boolean") syncBuiltInDaemon = stored.syncBuiltInDaemon;
    if (stored.account && canRemember())
      saved = JSON.parse(
        safeStorage.decryptString(Buffer.from(stored.account, "base64")),
      ) as SavedAccount;
  } catch {
    /* First run, unavailable keychain or invalid stored state: require login. */
  }
  let saveTail = Promise.resolve();
  const save = (value: SavedAccount | null): Promise<void> => {
    saved = value;
    const encoded =
      value && canRemember()
        ? safeStorage.encryptString(JSON.stringify(value)).toString("base64")
        : null;
    const syncPreference = syncBuiltInDaemon;
    const next = saveTail.then(async () => {
      await mkdir(path.dirname(file), { recursive: true });
      await writeFile(
        `${file}.tmp`,
        JSON.stringify({ installationId, account: encoded, syncBuiltInDaemon: syncPreference }),
        {
          mode: 0o600,
        },
      );
      await rename(`${file}.tmp`, file);
    });
    saveTail = next.catch(() => undefined);
    return next;
  };
  let transports: AccountTransportManager;
  let downloads: AccountDownloadManager;
  let hub: HubLink;
  const notify = (snapshot: AccountSnapshot) => {
    if (snapshot.status === "logged_out") hub?.stop();
    else hub?.start();
    for (const window of BrowserWindow.getAllWindows())
      if (isAppWindow(window.webContents))
        window.webContents.send("paseo:event:account-state", { ...snapshot, syncBuiltInDaemon });
  };
  const manager = new AccountSessionManager({
    installationId,
    appVersion: displayAppVersion(),
    browserLogin: desktopBrowserLogin((url) => shell.openExternal(url)),
    openUrl: (url) => shell.openExternal(url),
    save,
    closeTransports: () => {
      transports?.closeAll();
      downloads?.closeAll();
      browserProfiles?.closeConnections();
    },
    notify,
  });
  const snapshot = () => ({ ...manager.snapshot(), syncBuiltInDaemon });
  const setBuiltInHostSync = async (serverId: unknown, enabled: boolean) => {
    if (serverId !== getRuntime().status().serverId || syncBuiltInDaemon === enabled) return;
    const previous = syncBuiltInDaemon;
    syncBuiltInDaemon = enabled;
    try {
      await save(saved);
    } catch (error) {
      syncBuiltInDaemon = previous;
      throw error;
    }
    notify(manager.snapshot());
  };
  account = manager;
  hub = new HubLink({
    ticket: () => manager.openHubTicket(),
    webSocketFactory: (url, options) =>
      new WebSocket(url, options?.protocols, {
        headers: options?.headers,
        maxPayload: 1024 * 1024 + 4098,
        perMessageDeflate: false,
      }) as unknown as WebSocketLike,
    authentication: "bearer",
    randomUUID,
  });
  accountHub = hub;
  manager.attachHub(hub);
  const broadcast = (channel: string, payload: unknown) => {
    for (const window of BrowserWindow.getAllWindows())
      if (isAppWindow(window.webContents)) window.webContents.send(channel, payload);
  };
  hub.subscribe(() => broadcast("paseo:event:account-hub-state", hub.snapshot()));
  const hubBridge = new AccountHubBridge(hub);
  transports = new AccountTransportManager(manager, hub);
  browserProfiles = new AccountBrowserProfiles(manager, () => getRuntime().status().serverId, hub);
  downloads = new AccountDownloadManager(manager);
  const ready = saved ? manager.restore(saved).catch(() => save(null)) : save(null);
  let accountOperation = Promise.resolve<unknown>(undefined);
  return async (
    event: IpcMainInvokeEvent,
    command: string,
    args: Record<string, unknown> = {},
  ): Promise<unknown> => {
    if (event.senderFrame !== event.sender.mainFrame || !isAppWindow(event.sender)) {
      throw new Error("Account access requires an AIT application window");
    }
    await ready;
    const id = typeof args.sessionId === "string" ? args.sessionId : "";
    const operation = async (): Promise<unknown> => {
      switch (command) {
        case "account_status":
          return snapshot();
        case "account_info":
          return manager.getAccountInfo();
        case "account_hub_status":
          return hub.snapshot();
        case "account_hub_shares":
          if (typeof args.active !== "boolean") throw new Error("Invalid share subscription");
          hubBridge.setShareSubscription(event.sender, args.active);
          return null;
        case "account_hub_call": {
          return hubBridge.call(event.sender, args.method, args.params);
        }
        case "account_login_hosted": {
          if (args.center !== undefined && typeof args.center !== "string")
            throw new Error("Invalid service URL");
          await manager.loginWithBrowser((args.center as string) ?? "");
          const window = BrowserWindow.fromWebContents(event.sender);
          if (window && !window.isDestroyed()) {
            if (window.isMinimized()) window.restore();
            window.show();
            window.focus();
          }
          return snapshot();
        }
        case "account_cancel_login":
          manager.cancelLogin();
          return snapshot();
        case "account_logout":
          await manager.logout();
          hub.stop();
          return snapshot();
        case "account_delete":
          if (await manager.deleteAccount()) hub.stop();
          return snapshot();
        case "account_refresh":
          manager.refresh();
          return snapshot();
        case "account_select":
          await manager.select(typeof args.hostId === "string" ? args.hostId : null);
          return snapshot();
        case "account_host_sync": {
          // A delayed automatic request must never undo a user's manual stop.
          if (
            args.serverId === getRuntime().status().serverId &&
            !syncBuiltInDaemon &&
            args.enableBuiltInDaemon !== true
          )
            return null;
          if (args.enableBuiltInDaemon === true) await setBuiltInHostSync(args.serverId, true);
          const grant =
            args.needsAuthorization === true
              ? await manager.authorizeHost({
                  serverId: args.serverId as string,
                  instanceId: args.instanceId as string,
                  name: args.name as string,
                  platform: args.platform as string,
                })
              : null;
          notify(manager.snapshot());
          return grant;
        }
        case "account_host_disconnect":
          if (typeof args.serverId !== "string") throw new Error("Invalid host identity");
          await setBuiltInHostSync(args.serverId, false);
          notify(manager.snapshot());
          return;
        case "account_transport_open":
          if (typeof args.center !== "string") throw new Error("Invalid service URL");
          return transports.open(
            event.sender,
            id,
            typeof args.hostId === "string" ? args.hostId : "",
            args.center,
            typeof args.shareId === "string" && typeof args.nodeId === "string"
              ? { shareId: args.shareId, nodeId: args.nodeId }
              : undefined,
          );
        case "account_transport_send":
          return transports.send(event.sender, id, {
            ...(typeof args.text === "string" ? { text: args.text } : {}),
            ...(typeof args.binaryBase64 === "string" ? { binaryBase64: args.binaryBase64 } : {}),
          });
        case "account_transport_ack":
          transports.acknowledge(event.sender, id, Number(args.sequence));
          return;
        case "account_transport_close":
          transports.close(event.sender, id);
          return;
        case "account_transport_recover":
          return transports.recover(event.sender, id);
        case "account_browser_profile":
          if (typeof args.serverId !== "string") throw new Error("Invalid browser Host");
          return browserProfiles?.prepare(event.sender, {
            serverId: args.serverId,
            hostId: typeof args.hostId === "string" ? args.hostId : undefined,
            center: typeof args.center === "string" ? args.center : undefined,
            target: args.target,
            bearerToken: typeof args.bearerToken === "string" ? args.bearerToken : undefined,
          });
        case "account_download_prepare": {
          if (
            typeof args.hostId !== "string" ||
            typeof args.center !== "string" ||
            typeof args.fileName !== "string" ||
            typeof args.downloadId !== "string"
          )
            throw new Error("Invalid download request");
          return downloads.prepare(event.sender, {
            hostId: args.hostId,
            center: args.center,
            fileName: args.fileName,
            downloadId: args.downloadId,
          });
        }
        case "account_download": {
          if (typeof args.preparationId !== "string" || typeof args.token !== "string")
            throw new Error("Invalid download request");
          return downloads.download(event.sender, {
            preparationId: args.preparationId,
            token: args.token,
          });
        }
        case "account_download_cancel": {
          if (typeof args.preparationId !== "string") throw new Error("Invalid download request");
          downloads.cancel(event.sender, args.preparationId);
          return;
        }
        default:
          throw new Error("Unknown account command");
      }
    };
    // Serialize account changes; data frames and status reads remain independent.
    if (
      [
        "account_login_hosted",
        "account_logout",
        "account_delete",
        "account_select",
        "account_host_sync",
        "account_host_disconnect",
      ].includes(command)
    ) {
      const next = accountOperation.then(operation);
      accountOperation = next.catch(() => undefined);
      return next;
    }
    return operation();
  };
}
