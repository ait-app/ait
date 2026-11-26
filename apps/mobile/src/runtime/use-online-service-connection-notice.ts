import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { useToast } from "@/contexts/toast-api-context";
import { useAccountState } from "./account-state";
import { useHubServices } from "./hub-service-store";
import { useHostRuntimeClient, useHostRuntimeConnectionStatus } from "./host-runtime";
import { useActiveWorkspaceSelection } from "@/stores/navigation-active-workspace-store";

/** Keep automatic recovery neutral; notify each phase once without repeating on retries. */
export function useOnlineServiceConnectionNotice(): void {
  const { t } = useTranslation();
  const toast = useToast();
  const signedIn = useAccountState((state) => state.status !== "logged_out");
  const center = useAccountState((state) => state.center);
  const connection = useHubServices((state) => state.connection);
  const state = useHubServices((state) => state.snapshot?.state);
  const active = useActiveWorkspaceSelection();
  const hostStatus = useHostRuntimeConnectionStatus(active?.serverId ?? "");
  const client = useHostRuntimeClient(active?.serverId ?? "");
  const restoring = useSyncExternalStore(
    useCallback((listener) => client?.subscribeRecoveryStatus(listener) ?? (() => {}), [client]),
    () => client?.isRestoringSubscriptions ?? false,
  );
  const hostRecovering = Boolean(active && (hostStatus !== "online" || restoring));
  const recovering =
    signedIn && (hostRecovering || Boolean(state && state !== "online" && state !== "stopped"));
  const notice = useRef({ center, connection, shown: false, wasOnline: state === "online" });
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const dismiss = useRef<(() => void) | void>(undefined);

  useEffect(() => {
    if (notice.current.center !== center || notice.current.connection !== connection) {
      clearTimeout(timer.current);
      timer.current = undefined;
      dismiss.current?.();
      dismiss.current = undefined;
      notice.current = { center, connection, shown: false, wasOnline: state === "online" };
    }
    useHubServices.setState({ connectionRecovering: recovering });
    if (!recovering) {
      clearTimeout(timer.current);
      timer.current = undefined;
      dismiss.current?.();
      dismiss.current = undefined;
      notice.current.shown = false;
      if (state === "online") notice.current.wasOnline = true;
      return;
    }
    if (!notice.current.shown && !timer.current) {
      timer.current = setTimeout(() => {
        timer.current = undefined;
        notice.current.shown = true;
        dismiss.current = toast.show(
          t(notice.current.wasOnline ? "onlineService.reconnecting" : "onlineService.retrying"),
          { variant: "loading" },
        );
      }, 2_000);
    }
  }, [recovering, center, connection, state, t, toast]);

  useEffect(
    () => () => {
      clearTimeout(timer.current);
      timer.current = undefined;
      dismiss.current?.();
      dismiss.current = undefined;
      notice.current.shown = false;
      useHubServices.setState({ connectionRecovering: false });
    },
    [],
  );
}
