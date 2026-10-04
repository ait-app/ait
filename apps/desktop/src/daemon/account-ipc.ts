import { app, BrowserWindow, safeStorage, type IpcMainInvokeEvent } from "electron";
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { AccountSessionManager, type SavedAccount } from "./account-session.js";
import { AccountTransportManager } from "./account-transport.js";
import { AccountDownloadManager } from "./account-download.js";
import type { RustDaemonManager } from "./rust-daemon.js";

let account: AccountSessionManager | null = null;

function canRemember(): boolean {
  return (
    safeStorage.isEncryptionAvailable() &&
    (process.platform !== "linux" || safeStorage.getSelectedStorageBackend() !== "basic_text")
  );
}

export async function stopAccountForExit(): Promise<void> {
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
  try {
    const stored = JSON.parse(readFileSync(file, "utf8")) as {
      installationId?: string;
      account?: string;
    };
    if (typeof stored.installationId === "string" && /^[0-9a-f-]{36}$/i.test(stored.installationId))
      installationId = stored.installationId;
    if (stored.account && canRemember())
      saved = JSON.parse(
        safeStorage.decryptString(Buffer.from(stored.account, "base64")),
      ) as SavedAccount;
  } catch {
    /* First run, unavailable keychain or invalid stored state: require login. */
  }
  let saveTail = Promise.resolve();
  const save = (value: SavedAccount | null): Promise<void> => {
    const encoded =
      value && canRemember()
        ? safeStorage.encryptString(JSON.stringify(value)).toString("base64")
        : null;
    const next = saveTail.then(async () => {
      await mkdir(path.dirname(file), { recursive: true });
      await writeFile(`${file}.tmp`, JSON.stringify({ installationId, account: encoded }), {
        mode: 0o600,
      });
      await rename(`${file}.tmp`, file);
    });
    saveTail = next.catch(() => undefined);
    return next;
  };
  let transports: AccountTransportManager;
  let downloads: AccountDownloadManager;
  const manager = new AccountSessionManager({
    installationId,
    appVersion: app.getVersion(),
    publishRuntime: false,
    runtime: () => getRuntime().status(),
    local: (method, body) => getRuntime().relayRequest(method, body),
    save,
    closeTransports: () => {
      transports?.closeAll();
      downloads?.closeAll();
    },
    notify: (snapshot) => {
      for (const window of BrowserWindow.getAllWindows())
        if (isAppWindow(window.webContents))
          window.webContents.send("paseo:event:account-state", snapshot);
    },
  });
  account = manager;
  transports = new AccountTransportManager(manager);
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
          return manager.snapshot();
        case "account_login": {
          if (
            (args.center !== undefined && typeof args.center !== "string") ||
            typeof args.email !== "string" ||
            typeof args.password !== "string"
          )
            throw new Error("Invalid login");
          return manager.login(args.center ?? "", args.email, args.password);
        }
        case "account_logout":
          await manager.logout();
          return manager.snapshot();
        case "account_refresh":
          manager.refresh();
          return manager.snapshot();
        case "account_select":
          return manager.select(typeof args.hostId === "string" ? args.hostId : null);
        case "account_host_sync":
          return manager.publishHost(
            {
              serverId: args.serverId as string,
              instanceId: args.instanceId as string,
              name: args.name as string,
              platform: args.platform as string,
            },
            args.needsGrant !== false,
          );
        case "account_host_disconnect":
          if (typeof args.serverId !== "string") throw new Error("Invalid host identity");
          await manager.unpublishHost(args.serverId);
          return;
        case "account_transport_open":
          return transports.open(
            event.sender,
            id,
            typeof args.hostId === "string" ? args.hostId : "",
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
        case "account_download_prepare": {
          if (
            typeof args.hostId !== "string" ||
            typeof args.fileName !== "string" ||
            typeof args.downloadId !== "string"
          )
            throw new Error("Invalid download request");
          return downloads.prepare(event.sender, {
            hostId: args.hostId,
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
        "account_login",
        "account_logout",
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
