import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountDependencies, AccountSnapshot, SavedAccount } from "./account-session";
import type { RustDaemonManager } from "./rust-daemon";
import { createAccountIpc } from "./account-ipc";

const mocks = vi.hoisted(() => ({
  userDataPath: "",
  send: vi.fn(),
  publish: vi.fn(),
  unpublish: vi.fn(),
  info: vi.fn(),
  deleteAccount: vi.fn(),
  deps: null as AccountDependencies | null,
}));

vi.mock("electron", () => ({
  app: { getPath: () => mocks.userDataPath, getVersion: () => "test" },
  BrowserWindow: {
    getAllWindows: () => [{ webContents: { send: mocks.send } }],
    fromWebContents: () => null,
  },
  safeStorage: {
    isEncryptionAvailable: () => true,
    getSelectedStorageBackend: () => "keychain",
    encryptString: (value: string) => Buffer.from(value),
    decryptString: (value: Buffer) => value.toString(),
  },
  shell: { openExternal: vi.fn() },
}));
vi.mock("./account-session.js", () => ({
  AccountSessionManager: class {
    private status = "logged_out";
    constructor(private readonly deps: AccountDependencies) {
      mocks.deps = deps;
    }
    snapshot() {
      return { status: this.status, synchronizedHosts: [] };
    }
    attachHub() {}
    async restore() {
      this.status = "online";
    }
    async loginWithBrowser() {
      await this.deps.save({
        center: "https://example.test/api",
        token: "test-token",
        name: "Me",
        expiresAt: Date.now() + 3600000,
      });
      this.status = "online";
      this.deps.notify(this.snapshot() as AccountSnapshot);
    }
    async logout() {
      await this.deps.save(null);
      this.status = "logged_out";
    }
    async deleteAccount() {
      if ((await mocks.deleteAccount()) === false) return false;
      await this.logout();
      return true;
    }
    publishHost = mocks.publish;
    unpublishHost = mocks.unpublish;
    getAccountInfo = mocks.info;
  },
}));
vi.mock("./account-transport.js", () => ({ AccountTransportManager: class {} }));
vi.mock("./account-download.js", () => ({ AccountDownloadManager: class {} }));
vi.mock("./account-browser-login.js", () => ({ desktopBrowserLogin: vi.fn() }));

const runtime = { status: () => ({ serverId: "built-in" }) } as unknown as RustDaemonManager;
const frame = {};
const event = { senderFrame: frame, sender: { mainFrame: frame } } as Electron.IpcMainInvokeEvent;
const host = { serverId: "built-in", instanceId: "instance", name: "Desktop", platform: "darwin" };
const invoke = () =>
  createAccountIpc(
    () => runtime,
    () => true,
  );
const stored = () =>
  JSON.parse(readFileSync(path.join(mocks.userDataPath, "account-node.json"), "utf8"));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.userDataPath = mkdtempSync(path.join(tmpdir(), "ait-account-sync-"));
  mocks.publish.mockResolvedValue({ control_ticket: "one-use" });
  mocks.unpublish.mockResolvedValue(undefined);
});
afterEach(() => rmSync(mocks.userDataPath, { recursive: true, force: true }));

describe("desktop built-in daemon synchronization preference", () => {
  it("reads account information through the credential-owning manager", async () => {
    const info = {
      loginName: "me@example.test",
      expiresAt: null,
    };
    mocks.info.mockResolvedValue(info);
    await expect(invoke()(event, "account_info")).resolves.toEqual(info);
    expect(mocks.info).toHaveBeenCalledExactlyOnceWith();
  });
  it("defaults to automatic synchronization for new and existing desktop profiles", async () => {
    writeFileSync(
      path.join(mocks.userDataPath, "account-node.json"),
      JSON.stringify({ installationId: "00000000-0000-4000-8000-000000000001" }),
    );
    const command = invoke();
    await expect(command(event, "account_status")).resolves.toMatchObject({
      syncBuiltInDaemon: true,
    });
    expect(stored()).toMatchObject({ syncBuiltInDaemon: true });
    expect(mocks.deps?.publishRuntime).toBe(false);
  });

  it("routes hub serve control to the built-in daemon's local hub API", async () => {
    const hubRequest = vi.fn(async () => null);
    const command = createAccountIpc(
      () => ({ ...runtime, hubRequest }) as unknown as RustDaemonManager,
      () => true,
    );
    await command(event, "account_status");
    await mocks.deps!.localHub!("PUT", { hub_ticket: "ticket" });
    expect(hubRequest).toHaveBeenCalledWith("PUT", { hub_ticket: "ticket" });
  });

  it("retains a manual stop across logout, browser login and desktop restart", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    await command(event, "account_host_disconnect", { serverId: "built-in" });
    expect(mocks.unpublish).toHaveBeenCalledWith("built-in");
    expect(stored()).toMatchObject({ syncBuiltInDaemon: false });
    expect(mocks.send).toHaveBeenLastCalledWith(
      "paseo:event:account-state",
      expect.objectContaining({ syncBuiltInDaemon: false }),
    );
    await command(event, "account_logout");
    await expect(command(event, "account_login_hosted")).resolves.toMatchObject({
      syncBuiltInDaemon: false,
    });
    const restarted = invoke();
    await expect(restarted(event, "account_status")).resolves.toMatchObject({
      status: "online",
      syncBuiltInDaemon: false,
    });
  });

  it("restores the default when the built-in host is explicitly synchronized again", async () => {
    const command = invoke();
    await command(event, "account_host_disconnect", { serverId: "built-in" });
    await expect(
      command(event, "account_host_sync", { ...host, enableBuiltInDaemon: true }),
    ).resolves.toEqual({
      control_ticket: "one-use",
    });
    expect(mocks.publish).toHaveBeenCalledExactlyOnceWith(host, true);
    expect(stored()).toMatchObject({ syncBuiltInDaemon: true });
    expect(mocks.send).toHaveBeenLastCalledWith(
      "paseo:event:account-state",
      expect.objectContaining({ syncBuiltInDaemon: true }),
    );
  });

  it("does not change the built-in preference when other hosts are synchronized or stopped", async () => {
    const command = invoke();
    await command(event, "account_host_disconnect", { serverId: "built-in" });
    await command(event, "account_host_sync", { ...host, serverId: "remote" });
    await expect(command(event, "account_status")).resolves.toMatchObject({
      syncBuiltInDaemon: false,
    });
    await command(event, "account_host_sync", { ...host, enableBuiltInDaemon: true });
    await command(event, "account_host_disconnect", { serverId: "remote" });
    await expect(command(event, "account_status")).resolves.toMatchObject({
      syncBuiltInDaemon: true,
    });
  });

  it("keeps the preference when account credentials are cleared", async () => {
    const command = invoke();
    await command(event, "account_host_disconnect", { serverId: "built-in" });
    await mocks.deps!.save(null);
    expect(stored()).toMatchObject({ account: null, syncBuiltInDaemon: false });
    await mocks.deps!.save({
      center: "https://example.test/api",
      token: "saved-token",
      name: "Me",
      expiresAt: Date.now() + 3600000,
    } satisfies SavedAccount);
    expect(stored().syncBuiltInDaemon).toBe(false);
    await expect(invoke()(event, "account_status")).resolves.toMatchObject({
      syncBuiltInDaemon: false,
    });
  });

  it("rejects delayed automatic publication after a manual stop, including when revocation fails", async () => {
    const command = invoke();
    mocks.unpublish.mockRejectedValueOnce(new Error("Service unreachable"));
    await expect(
      command(event, "account_host_disconnect", { serverId: "built-in" }),
    ).rejects.toThrow("Service unreachable");
    await expect(command(event, "account_host_sync", host)).resolves.toBeNull();
    expect(mocks.publish).not.toHaveBeenCalled();
    expect(stored().syncBuiltInDaemon).toBe(false);
  });
});

describe("desktop account login commands", () => {
  it("deletes through the credential owner and returns the signed-out state", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    await expect(command(event, "account_delete")).resolves.toMatchObject({ status: "logged_out" });
    expect(mocks.deleteAccount).toHaveBeenCalledExactlyOnceWith();
    expect(stored().account).toBeNull();
  });

  it("opens browser confirmation in the system browser and keeps the session", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    const saved = stored();
    mocks.deleteAccount.mockResolvedValueOnce(false);
    await expect(command(event, "account_delete")).resolves.toMatchObject({ status: "online" });
    expect(stored()).toEqual(saved);
    const { shell } = await import("electron");
    await mocks.deps?.openUrl?.("https://example.test/api/v1/auth/delete/authorize");
    expect(shell.openExternal).toHaveBeenCalledWith(
      "https://example.test/api/v1/auth/delete/authorize",
    );
  });

  it("retains credentials when deletion is rejected", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    const saved = stored();
    mocks.deleteAccount.mockRejectedValueOnce(new Error("Service unavailable"));
    await expect(command(event, "account_delete")).rejects.toThrow("Service unavailable");
    expect(stored()).toEqual(saved);
    await expect(command(event, "account_status")).resolves.toMatchObject({ status: "online" });
  });

  it("serializes other account changes behind deletion", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    let finish!: () => void;
    mocks.deleteAccount.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const deletion = command(event, "account_delete");
    await vi.waitFor(() => expect(mocks.deleteAccount).toHaveBeenCalledOnce());
    const publication = command(event, "account_host_sync", host);
    await command(event, "account_status");
    expect(mocks.publish).not.toHaveBeenCalled();
    finish();
    await deletion;
    await publication;
    expect(mocks.publish).toHaveBeenCalledOnce();
  });

  it("rejects the removed password command without replacing the current account", async () => {
    const command = invoke();
    await command(event, "account_login_hosted");
    const saved = stored();
    mocks.send.mockClear();
    await expect(
      command(event, "account_login", {
        center: "https://other.test/api",
        email: "me@example.test",
        password: "test-password",
      }),
    ).rejects.toThrow("Unknown account command");
    expect(stored()).toEqual(saved);
    expect(mocks.send).not.toHaveBeenCalled();
    await expect(command(event, "account_status")).resolves.toMatchObject({ status: "online" });
  });
});
