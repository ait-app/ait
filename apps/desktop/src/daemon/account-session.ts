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
    super({ ...deps, deviceName: hostname(), platform: process.platform, randomUUID });
  }
}
