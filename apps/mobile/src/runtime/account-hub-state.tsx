import { useEffect, useRef } from "react";
import * as Crypto from "expo-crypto";
import { usePathname } from "expo-router";
import { observeHubEvents } from "@ait/client/internal/hub";
import { useAccountState } from "./account-state";
import { connectAccountHub } from "./account-hub";
import { loadHubAgentSummaries, hubAttentionNotification } from "./hub-services";
import { useAppVisible } from "@/hooks/use-app-visible";
import { useSessionStore } from "@/stores/session-store";
import { parseServerIdFromPathname } from "@/utils/host-routes";
import { sendOsNotification } from "@/utils/os-notifications";
import { claimAttentionNotification } from "@/utils/attention-notification-dedup";
import { isWeb } from "@/constants/platform";
import { startAccountPushNotifications } from "@/push-notifications";

import { useHubServices } from "./hub-service-store";

/** One account-wide consumer, independent of which daemon views are mounted or connected. */
export function AccountHubLifecycle() {
  const signedIn = useAccountState((state) => state.status !== "logged_out");
  const center = useAccountState((state) => state.center);
  const visible = useAppVisible();
  const pathname = usePathname();
  const serverId = parseServerIdFromPathname(pathname);
  const focusedAgentId = useSessionStore((state) =>
    serverId ? (state.sessions[serverId]?.focusedAgentId ?? null) : null,
  );
  const focusedTerminalId = useSessionStore((state) =>
    serverId ? (state.sessions[serverId]?.focusedTerminalId ?? null) : null,
  );
  const connection = useHubServices((state) => state.connection);
  const linkId = useHubServices((state) => state.snapshot?.linkId);
  const focus = useRef({ visible, serverId, focusedAgentId, focusedTerminalId });
  focus.current = { visible, serverId, focusedAgentId, focusedTerminalId };
  const eventsActive = useHubServices((state) => state.eventsActive);
  const focusedHostId = useAccountState(
    (state) => state.hosts.find((host) => host.server_id === serverId)?.host_id,
  );
  const online = useHubServices((state) => state.snapshot?.state === "online");

  useEffect(() => {
    if (!signedIn) return;
    let disposed = false;
    let cleanup = () => {};
    void connectAccountHub()
      .then((handle) => {
        if (!handle) return;
        if (disposed) {
          handle.dispose();
          return;
        }
        const { connection: hub } = handle;
        let busy: Promise<void> | undefined;
        let refreshAgain = false;
        const refresh = (): Promise<void> => {
          if (disposed) return Promise.resolve();
          if (busy) {
            refreshAgain = true;
            return busy;
          }
          busy = (async () => {
            do {
              refreshAgain = false;
              const identity = hub.snapshot().linkId;
              const agents = await loadHubAgentSummaries(hub);
              if (!disposed && hub.snapshot().linkId === identity)
                useHubServices.setState({ agents });
            } while (refreshAgain && !disposed);
          })().finally(() => {
            busy = undefined;
          });
          return busy;
        };
        useHubServices.setState({ connection: hub, snapshot: hub.snapshot() });
        const removeState = hub.subscribe(() =>
          useHubServices.setState({ snapshot: hub.snapshot() }),
        );
        let refreshTimer: ReturnType<typeof setTimeout> | undefined;
        const removeEvents = observeHubEvents(hub, {
          randomUUID: Crypto.randomUUID,
          events: [
            "agent_attention_required",
            "terminal_attention_required",
            "agent.update",
            "server.status",
            "status.server_info",
            "creation.update",
          ],
          snapshot: refresh,
          active: (eventsActive) => {
            if (!disposed) useHubServices.setState({ eventsActive });
          },
          event: async (event) => {
            if (disposed) return;
            const account = useAccountState.getState();
            const host = account.hosts.find((value) => value.host_id === event.hostId);
            const notification = host?.server_id
              ? hubAttentionNotification(event, host.server_id, useHubServices.getState().agents)
              : null;
            if (notification) {
              const current = focus.current;
              const focused =
                current.visible &&
                current.serverId === host?.server_id &&
                ((notification.agentId && current.focusedAgentId === notification.agentId) ||
                  (notification.terminalId &&
                    current.focusedTerminalId === notification.terminalId));
              if (
                !focused &&
                !event.notificationSuppressed &&
                claimAttentionNotification(
                  notification.key,
                  notification.terminalId ? 5000 : undefined,
                )
              )
                await sendOsNotification(notification);
            }
            if (
              ["agent.update", "server.status", "status.server_info", "creation.update"].includes(
                event.method,
              )
            )
              refreshTimer ??= setTimeout(() => {
                refreshTimer = undefined;
                void refresh().catch(() => {});
              }, 750);
          },
          error: () => {
            if (!disposed) console.warn("[Hub] Account event subscription unavailable; retrying");
          },
        });
        const stopPush = startAccountPushNotifications({ connection: hub, key: center });
        cleanup = () => {
          clearTimeout(refreshTimer);
          stopPush();
          removeEvents();
          removeState();
          handle.dispose();
        };
      })
      .catch(() => {
        if (!disposed) console.warn("[Hub] Account event connection unavailable");
      });
    return () => {
      disposed = true;
      cleanup();
      useHubServices.setState({
        connection: null,
        snapshot: null,
        agents: [],
        eventsActive: false,
      });
    };
  }, [signedIn, center]);

  useEffect(() => {
    if (
      !connection ||
      !online ||
      !signedIn ||
      !connection.snapshot().methods.includes("session.heartbeat")
    )
      return;
    // The route determines focus; background Host providers cannot overwrite the active Host.
    const heartbeat = () =>
      void connection
        .call("session.heartbeat", {
          appVisible: visible && eventsActive,
          deviceType: isWeb ? "web" : "mobile",
          ...(focusedHostId ? { focusedHostId, focusedAgentId, focusedTerminalId } : {}),
        })
        .catch(() => {});
    heartbeat();
    const timer = setInterval(heartbeat, 15_000);
    return () => clearInterval(timer);
  }, [
    connection,
    online,
    linkId,
    signedIn,
    visible,
    eventsActive,
    focusedHostId,
    focusedAgentId,
    focusedTerminalId,
  ]);
  return null;
}
