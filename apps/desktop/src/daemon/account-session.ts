import type { WebSocketLike } from "@ait/client/internal/daemon-client-transport-types";
import { WebSocket } from "ws";
import { createWebSocketTransportFactory } from "@ait/client/internal/daemon-client-websocket-transport";
import { randomUUID } from "node:crypto";
import { hostname } from "node:os";
import {
  AccountSessionManager as SharedAccountSessionManager,
  type AccountDependencies as SharedAccountDependencies,
} from "@ait/client/internal/account-session";

export {
  AccountError,
  DEFAULT_ACCOUNT_CENTER,
  normalizeCenter,
} from "@ait/client/internal/account-session";
export type {
  AccountHost,
  AccountSnapshot,
  SavedAccount,
} from "@ait/client/internal/account-session";
export type AccountDependencies = Omit<
  SharedAccountDependencies,
  "deviceName" | "platform" | "randomUUID"
>;

/** Electron owns credentials and supplies the local runtime to the shared account lifecycle. */
export class AccountSessionManager extends SharedAccountSessionManager {
  constructor(deps: AccountDependencies) {
    super({
      channelTransport: createWebSocketTransportFactory(
        (url, options) =>
          new WebSocket(url, options?.protocols, {
            headers: options?.headers,
          }) as unknown as WebSocketLike,
      ),
      channelAuthentication: "bearer",
      ...deps,
      deviceName: hostname(),
      platform: process.platform,
      randomUUID,
    });
  }
}
