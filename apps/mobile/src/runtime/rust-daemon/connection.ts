import { buildDesktopDaemonTransportUrl } from "@/desktop/daemon/desktop-daemon-transport";
import type { DesktopDaemonTransportTarget } from "@/desktop/daemon/desktop-daemon";
import { createWebSocketTransportFactory } from "@ait/client/internal/daemon-client-websocket-transport";
import { buildDaemonWebSocketUrl } from "@/utils/daemon-endpoints";
import { isWeb } from "@/constants/platform";
import { createAppWebSocketFactory } from "../websocket-factory";
import { createRustDaemonTransportFactory } from "./transport";
import { createBrowserRustTransportFactory } from "./browser-transport";
import type { TransportFactory } from "./types";
import { createAccountRelayTransportFactory } from "./account-transport";

export function buildAccountRelayClientConfig(connection: { hostId: string; center: string }) {
  const url = new URL(`ait+desktop://account-relay/${connection.hostId}`);
  url.searchParams.set("center", connection.center);
  return {
    url: url.toString(),
    connectTimeoutMs: 45_000,
    transportFactory: createRustDaemonTransportFactory(createAccountRelayTransportFactory),
  };
}

/** Shared by the host probe and the long-lived runtime; password is the Rust Bearer token. */
export function buildRustClientConfig(
  connection: { endpoint: string; useTls?: boolean; password?: string },
  desktopTransport: TransportFactory | null,
) {
  const url = new URL(
    buildDaemonWebSocketUrl(connection.endpoint, {
      useTls: connection.useTls ?? false,
    }),
  );
  url.pathname = "/v1/ws";
  const nativeTransport = createWebSocketTransportFactory(createAppWebSocketFactory());
  const base: TransportFactory =
    desktopTransport ?? (isWeb ? createBrowserRustTransportFactory() : nativeTransport);
  return {
    url: url.toString(),
    ...(connection.password ? { password: connection.password } : {}),
    transportFactory: createRustDaemonTransportFactory(base),
  };
}

/** SSH carries the same Rust protocol and Bearer authentication as direct TCP. */
export function buildRustSshClientConfig(
  connection: { host: string; sshPort?: number; daemonPort?: number; password?: string },
  desktopTransport: TransportFactory | null | undefined,
  buildUrl: (target: DesktopDaemonTransportTarget) => string = buildDesktopDaemonTransportUrl,
) {
  if (!desktopTransport) throw new Error("Remote SSH is only available in the desktop app.");
  if (!connection.password || !/^[\x21-\x7e]{32,256}$/.test(connection.password)) {
    throw new Error("Rust daemon requires a 32–256 character Bearer token");
  }
  return {
    url: buildUrl({
      transportType: "ssh",
      host: connection.host,
      ...(connection.sshPort !== undefined ? { sshPort: connection.sshPort } : {}),
      ...(connection.daemonPort !== undefined ? { daemonPort: connection.daemonPort } : {}),
    }),
    password: connection.password,
    transportFactory: createRustDaemonTransportFactory(desktopTransport),
  };
}
