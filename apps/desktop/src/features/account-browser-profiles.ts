import { createHash } from "node:crypto";
import { readdirSync } from "node:fs";
import path from "node:path";
import { app, session, type Session, type WebContents } from "electron";
import { AccountTransportManager } from "../daemon/account-transport.js";
import type { AccountSessionManager } from "../daemon/account-session.js";
import { BrowserRelayNetwork, browserProxyConfig } from "./browser-network.js";

const PREFIX = "ait-browser-relay-";
interface Profile {
  owners: Set<number>;
  network: BrowserRelayNetwork;
  ready: Promise<{ profilePartition: string }>;
}

/** Owns per-Host browser routing without exposing account credentials to guests. */
export class AccountBrowserProfiles {
  private profiles = new Map<string, Profile>();
  private transports: AccountTransportManager;

  constructor(account: AccountSessionManager) {
    this.transports = new AccountTransportManager(account);
  }

  async prepare(owner: WebContents, input: { hostId: string; center: string; serverId: string }) {
    if (!/^[0-9a-f-]{36}$/i.test(input.hostId) || !/^[0-9a-f-]{36}$/i.test(input.serverId))
      throw new Error("Invalid browser Host identity");
    const hash = createHash("sha256")
      .update(JSON.stringify([input.center, input.hostId, input.serverId]))
      .digest("hex");
    const partition = `persist:${PREFIX}${hash}`;
    let profile = this.profiles.get(partition);
    if (!profile) {
      if (this.profiles.size >= 64) throw new Error("Too many remote browser profiles");
      const network = new BrowserRelayNetwork(this.transports);
      profile = {
        owners: new Set(),
        network,
        ready: Promise.resolve({ profilePartition: partition }),
      };
      const current = profile;
      this.profiles.set(partition, current);
      current.ready = network
        .listen(input.hostId, input.center, input.serverId)
        .then(async (port) => {
          await session.fromPartition(partition).setProxy(browserProxyConfig(port));
          return { profilePartition: partition };
        })
        .catch((error: unknown) => {
          network.close();
          this.profiles.delete(partition);
          throw error;
        });
    }
    if (!profile.owners.has(owner.id)) {
      profile.owners.add(owner.id);
      const current = profile;
      owner.once("destroyed", () => {
        current.owners.delete(owner.id);
        if (!current.owners.size) {
          current.network.close();
          this.profiles.delete(partition);
        }
      });
    }
    return profile.ready;
  }

  partitions(ownerId: number): string[] {
    return [...this.profiles]
      .filter(([, profile]) => profile.owners.has(ownerId))
      .map(([name]) => name);
  }

  closeConnections(): void {
    this.transports.closeAll();
  }
}

/** Include dormant persistent remote profiles when clearing browser data. */
export function remoteBrowserProfileSessions(): Session[] {
  try {
    return readdirSync(path.join(app.getPath("sessionData"), "Partitions"))
      .filter((name) => /^ait-browser-relay-[a-f0-9]{64}$/.test(name))
      .map((name) => session.fromPartition(`persist:${name}`));
  } catch {
    return [];
  }
}
