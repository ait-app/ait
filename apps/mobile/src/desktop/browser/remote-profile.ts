import { getDesktopHost, type DesktopBrowserBridge } from "@/desktop/host";
import type { BrowserWebviewProfileHost } from "./resident-webviews";
import type { DesktopDaemonTransportTarget } from "@/desktop/daemon/desktop-daemon";
import { buildDaemonWebSocketUrl } from "@/utils/daemon-endpoints";

/** Resolve the selected Workspace Host before the first guest navigation. */
export async function resolveBrowserProfileHost(
  serverId: string,
  browser: DesktopBrowserBridge | undefined = getDesktopHost()?.browser,
): Promise<BrowserWebviewProfileHost | undefined> {
  if (!browser?.prepareWorkspaceProfile || !browser.registerAttachedBrowser) return undefined;
  const { getHostRuntimeStore } = await import("@/runtime/host-runtime");
  const store = getHostRuntimeStore();
  if (!store.isHostRegistryLoaded()) await store.boot();
  const snapshot = store.getSnapshot(serverId);
  const host = store.getHosts().find((host) => host.serverId === serverId);
  if (!host) throw new Error("Workspace Host is not available for browser routing");
  const connection =
    host?.connections.find((connection) => connection.id === snapshot?.activeConnectionId) ??
    host?.connections.find((connection) => connection.id === host.preferredConnectionId) ??
    host?.connections[0];
  if (!connection) throw new Error("Workspace Host has no browser route");
  let target: DesktopDaemonTransportTarget | undefined;
  switch (connection.type) {
    case "directTcp": {
      const url = new URL(
        buildDaemonWebSocketUrl(connection.endpoint, { useTls: connection.useTls ?? false }),
      );
      url.pathname = "/v1/ws";
      target = { transportType: "rustTcp", url: url.toString() };
      break;
    }
    case "remoteSsh":
      target = {
        transportType: "ssh",
        host: connection.host,
        sshPort: connection.sshPort,
        daemonPort: connection.daemonPort,
      };
      break;
    case "directSocket":
      target = { transportType: "socket", transportPath: connection.path };
      break;
    case "directPipe":
      target = { transportType: "pipe", transportPath: connection.path };
      break;
    case "accountRelay":
      break;
  }
  const profile = await browser.prepareWorkspaceProfile(
    connection.type === "accountRelay"
      ? {
          serverId,
          hostId: connection.hostId,
          center: connection.center,
        }
      : {
          serverId,
          target,
          bearerToken:
            connection.type === "directTcp" || connection.type === "remoteSsh"
              ? connection.password
              : undefined,
        },
  );
  return {
    profilePartition: profile.profilePartition,
    registerAttachedBrowser: browser.registerAttachedBrowser,
  };
}
