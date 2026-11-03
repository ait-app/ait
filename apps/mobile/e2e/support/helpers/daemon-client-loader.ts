import { DaemonClient } from "@ait/client/internal/daemon-client";
import { createWebSocketTransportFactory } from "@ait/client/internal/daemon-client-websocket-transport";
import * as protocolSchemas from "@ait/protocol/messages";
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { requireAitDaemon } from "./ait-daemon";
import { getE2EDaemonPort } from "./daemon-port";
import { createNodeWebSocketFactory } from "./node-ws-factory";

export interface ConnectDaemonClientOptions {
  clientIdPrefix: string;
  appVersion?: string;
  port?: number;
}

/**
 * Connects an in-test daemon client over the isolated E2E daemon's WebSocket.
 * The port-6767 guard keeps tests off the developer daemon. Each helper passes
 * its own typed client interface as the generic.
 */
export async function connectDaemonClient<
  ClientInstance extends { connect(): Promise<void>; close(): Promise<void> },
>(options: ConnectDaemonClientOptions): Promise<ClientInstance> {
  const connection = requireAitDaemon(options.port ?? Number(getE2EDaemonPort()));
  const client = new DaemonClient({
    url: `ws://127.0.0.1:${connection.port}/v1/ws`,
    password: connection.token,
    clientId: `${options.clientIdPrefix}-${randomUUID()}`,
    clientType: "cli",
    appVersion: options.appVersion ?? loadAppVersion(),
    transportFactory: createWebSocketTransportFactory(createNodeWebSocketFactory()),
  });
  try {
    await client.connect();
    return client as unknown as ClientInstance;
  } catch (error) {
    await client.close().catch(() => undefined);
    throw error;
  }
}

function loadAppVersion(): string {
  const packageJsonPath = path.resolve(__dirname, "../../../package.json");
  const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8")) as { version?: unknown };
  if (typeof packageJson.version !== "string" || packageJson.version.length === 0) {
    throw new Error(`Missing app version in ${packageJsonPath}`);
  }
  return packageJson.version;
}

export async function loadProtocolSchemas(): Promise<typeof protocolSchemas> {
  return protocolSchemas;
}
