import { getDesktopHost, type DesktopBrowserBridge } from "@/desktop/host";
import type { BrowserWebviewProfileHost } from "./resident-webviews";

/** Resolve the selected Workspace Host before the first guest navigation. */
export async function resolveBrowserProfileHost(
  serverId: string,
  browser: DesktopBrowserBridge | undefined = getDesktopHost()?.browser,
): Promise<BrowserWebviewProfileHost | undefined> {
  if (!browser?.prepareAccountProfile || !browser.registerAttachedBrowser) return undefined;
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
  if (connection?.type !== "accountRelay") return undefined;
  const profile = await browser.prepareAccountProfile({
    serverId,
    hostId: connection.hostId,
    center: connection.center,
  });
  return {
    profilePartition: profile.profilePartition,
    registerAttachedBrowser: browser.registerAttachedBrowser,
  };
}
