import type { DesktopDaemonTransportTarget } from "@/desktop/daemon/desktop-daemon";
import {
  buildDesktopDaemonTransportUrl,
  createDesktopDaemonTransportFactory,
} from "@/desktop/daemon/desktop-daemon-transport";
import {
  buildAccountRelayClientConfig,
  buildRustClientConfig,
  buildRustSshClientConfig,
} from "@/runtime/rust-daemon/connection";
import type { HostConnection } from "@/types/host-connection";
import type { DaemonClientConfig } from "@ait/client/internal/daemon-client";
import { DaemonClient } from "@ait/client/internal/daemon-client";
import { resolveAppVersion } from "./app-version";
import { getOrCreateClientId } from "./client-id";

export interface DaemonProbeClient {
  readonly lastError: string | null;
  connect(): Promise<void>;
  close(): Promise<void>;
  getLastServerInfoMessage(): { serverId: string; hostname: string | null } | null;
}

export interface DaemonConnectionDependencies<TClient extends DaemonProbeClient> {
  getClientId(): Promise<string>;
  resolveAppVersion(): string | null;
  createDesktopTransportFactory(): DaemonClientConfig["transportFactory"] | null;
  buildDesktopTransportUrl(input: DesktopDaemonTransportTarget): string;
  createClient(config: DaemonClientConfig): TClient;
}

const defaultDaemonConnectionDependencies: DaemonConnectionDependencies<DaemonClient> = {
  getClientId: getOrCreateClientId,
  resolveAppVersion,
  createDesktopTransportFactory: createDesktopDaemonTransportFactory,
  buildDesktopTransportUrl: buildDesktopDaemonTransportUrl,
  createClient: (config) => new DaemonClient(config),
};

function normalizeNonEmptyString(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function pickBestReason(reason: string | null, lastError: string | null): string {
  const genericReason =
    reason &&
    (reason.toLowerCase() === "transport error" || reason.toLowerCase() === "transport closed");
  const genericLastError =
    lastError &&
    (lastError.toLowerCase() === "transport error" ||
      lastError.toLowerCase() === "transport closed" ||
      lastError.toLowerCase() === "unable to connect");

  if (genericReason && lastError && !genericLastError) {
    return lastError;
  }
  if (reason) return reason;
  if (lastError) return lastError;
  return "Unable to connect";
}

function isIncorrectPasswordFailure(input: {
  config: DaemonClientConfig;
  reason: string | null;
  lastError: string | null;
}): boolean {
  if (!input.config.password) {
    return false;
  }
  const details = [input.reason, input.lastError].filter(Boolean).join("\n").toLowerCase();
  return (
    details.includes("401") ||
    details.includes("4001") ||
    details.includes("unauthorized") ||
    details.includes("code 1006")
  );
}

export class DaemonConnectionTestError extends Error {
  reason: string | null;
  lastError: string | null;

  constructor(message: string, details: { reason: string | null; lastError: string | null }) {
    super(message);
    this.name = "DaemonConnectionTestError";
    this.reason = details.reason;
    this.lastError = details.lastError;
  }
}

export async function buildClientConfig(
  connection: HostConnection,
  serverId?: string,
  options?: {
    capabilities?: DaemonClientConfig["capabilities"];
    trace?: DaemonClientConfig["trace"];
  },
  deps: Pick<
    DaemonConnectionDependencies<DaemonProbeClient>,
    | "getClientId"
    | "resolveAppVersion"
    | "createDesktopTransportFactory"
    | "buildDesktopTransportUrl"
  > = defaultDaemonConnectionDependencies,
): Promise<DaemonClientConfig> {
  const clientId = await deps.getClientId();
  const desktopTransportFactory = deps.createDesktopTransportFactory();
  const base = {
    clientId,
    clientType: "mobile" as const,
    appVersion: deps.resolveAppVersion() ?? undefined,
    suppressSendErrors: true,
    reconnect: { enabled: false },
    ...(options?.capabilities ? { capabilities: options.capabilities } : {}),
    ...(options?.trace ? { trace: options.trace } : {}),
    ...((connection.type === "directSocket" || connection.type === "directPipe") &&
    desktopTransportFactory
      ? { transportFactory: desktopTransportFactory }
      : {}),
  };

  if (connection.type === "directSocket" || connection.type === "directPipe") {
    return {
      ...base,
      url: deps.buildDesktopTransportUrl({
        transportType: connection.type === "directSocket" ? "socket" : "pipe",
        transportPath: connection.path,
      }),
    };
  }

  if (connection.type === "remoteSsh") {
    return {
      ...base,
      ...buildRustSshClientConfig(
        connection,
        desktopTransportFactory,
        deps.buildDesktopTransportUrl,
      ),
    };
  }

  if (connection.type === "accountRelay")
    return { ...base, ...buildAccountRelayClientConfig(connection) };
  if (connection.type === "directTcp") {
    return {
      ...base,
      ...buildRustClientConfig(connection, desktopTransportFactory ?? null),
    };
  }

  throw new Error("Unsupported host connection");
}

export function connectAndProbe(
  config: DaemonClientConfig,
  timeoutMs: number,
): Promise<{ client: DaemonClient; serverId: string; hostname: string | null }>;
export function connectAndProbe<TClient extends DaemonProbeClient>(
  config: DaemonClientConfig,
  timeoutMs: number,
  deps: Pick<DaemonConnectionDependencies<TClient>, "createClient">,
): Promise<{ client: TClient; serverId: string; hostname: string | null }>;
export function connectAndProbe(
  config: DaemonClientConfig,
  timeoutMs: number,
  deps: Pick<
    DaemonConnectionDependencies<DaemonProbeClient>,
    "createClient"
  > = defaultDaemonConnectionDependencies,
): Promise<{ client: DaemonProbeClient; serverId: string; hostname: string | null }> {
  const client = deps.createClient(config);

  return new Promise<{ client: DaemonProbeClient; serverId: string; hostname: string | null }>(
    (resolve, reject) => {
      const timer = setTimeout(() => {
        void client.close().catch(() => undefined);
        reject(
          new DaemonConnectionTestError("Connection timed out", {
            reason: "Connection timed out",
            lastError: client.lastError ?? null,
          }),
        );
      }, timeoutMs);

      void client
        .connect()
        .then(() => {
          clearTimeout(timer);
          const serverInfo = client.getLastServerInfoMessage();
          if (!serverInfo) {
            void client.close().catch(() => undefined);
            reject(
              new DaemonConnectionTestError("Missing server info message", {
                reason: "Missing server info message",
                lastError: client.lastError ?? null,
              }),
            );
            return;
          }
          resolve({
            client,
            serverId: serverInfo.serverId,
            hostname: serverInfo.hostname,
          });
          return;
        })
        .catch((error) => {
          clearTimeout(timer);
          const reason = normalizeNonEmptyString(
            error instanceof Error ? error.message : String(error),
          );
          const lastError = normalizeNonEmptyString(client.lastError);
          const message = isIncorrectPasswordFailure({ config, reason, lastError })
            ? "Incorrect password"
            : pickBestReason(reason, lastError);
          void client.close().catch(() => undefined);
          reject(new DaemonConnectionTestError(message, { reason, lastError }));
        });
    },
  );
}

interface ProbeOptions {
  serverId?: string;
  timeoutMs?: number;
  capabilities?: DaemonClientConfig["capabilities"];
  trace?: DaemonClientConfig["trace"];
}

function resolveTimeout(connection: HostConnection, options?: ProbeOptions): number {
  if (options?.timeoutMs) return options.timeoutMs;
  if (connection.type === "accountRelay") return 45_000;
  if (connection.type === "remoteSsh") return 15_000;
  return 6_000;
}

export function connectToDaemon(
  connection: HostConnection,
  options?: ProbeOptions,
): Promise<{ client: DaemonClient; serverId: string; hostname: string | null }>;
export function connectToDaemon<TClient extends DaemonProbeClient>(
  connection: HostConnection,
  options: ProbeOptions | undefined,
  deps: DaemonConnectionDependencies<TClient>,
): Promise<{ client: TClient; serverId: string; hostname: string | null }>;
export async function connectToDaemon(
  connection: HostConnection,
  options?: ProbeOptions,
  deps: DaemonConnectionDependencies<DaemonProbeClient> = defaultDaemonConnectionDependencies,
): Promise<{ client: DaemonProbeClient; serverId: string; hostname: string | null }> {
  const config = await buildClientConfig(connection, options?.serverId, options, deps);
  return connectAndProbe(config, resolveTimeout(connection, options), deps);
}
