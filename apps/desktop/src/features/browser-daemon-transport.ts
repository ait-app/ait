import type { AccountTransportOwner } from "../daemon/account-transport.js";
import {
  createMainProcessTransportManager,
  type TransportTarget,
} from "../daemon/local-transport.js";

/** Reuses authenticated TCP/SSH transports without broadcasting browser traffic to renderers. */
export class BrowserDaemonTransport {
  private owner: AccountTransportOwner | null = null;
  private sessionId: string | null = null;
  private readonly manager = createMainProcessTransportManager((payload) => {
    if (payload.sessionId === this.sessionId) this.owner?.send("browser-daemon", { ...payload });
  });

  constructor(private readonly input: { target: TransportTarget; bearerToken?: string }) {}

  async open(owner: AccountTransportOwner, id: string): Promise<void> {
    this.owner = owner;
    this.sessionId = id;
    this.manager.open({ ...this.input, sessionId: id });
  }

  async send(_owner: AccountTransportOwner, id: string, input: { text?: string }): Promise<void> {
    await this.manager.send({ ...input, sessionId: id });
  }

  acknowledge(): void {}

  close(_owner: AccountTransportOwner, id: string): void {
    this.manager.close(id);
    this.owner = null;
    this.sessionId = null;
  }
}
