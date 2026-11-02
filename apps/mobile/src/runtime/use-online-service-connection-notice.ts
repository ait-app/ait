import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useToast } from "@/contexts/toast-api-context";
import { useAccountState } from "./account-state";
import { useHubServices } from "./hub-service-store";

/** Keep automatic recovery neutral; notify each phase once without repeating on retries. */
export function useOnlineServiceConnectionNotice(): void {
  const { t } = useTranslation();
  const toast = useToast();
  const signedIn = useAccountState((state) => state.status !== "logged_out");
  const center = useAccountState((state) => state.center);
  const connection = useHubServices((state) => state.connection);
  const state = useHubServices((state) => state.snapshot?.state);
  const notice = useRef({ center, connection, reconnectingShown: false, unavailableShown: false });

  useEffect(() => {
    if (notice.current.center !== center || notice.current.connection !== connection) {
      notice.current = { center, connection, reconnectingShown: false, unavailableShown: false };
    }
    if (!signedIn || !state || state === "online" || state === "stopped") {
      notice.current.reconnectingShown = false;
      notice.current.unavailableShown = false;
      return;
    }
    if (
      state === "reconnecting" &&
      !notice.current.reconnectingShown &&
      !notice.current.unavailableShown
    ) {
      notice.current.reconnectingShown = true;
      toast.show(t("onlineService.reconnecting"), { variant: "loading" });
    } else if (state === "unavailable" && !notice.current.unavailableShown) {
      notice.current.unavailableShown = true;
      toast.show(t("onlineService.retrying"), { variant: "loading", durationMs: 3200 });
    }
  }, [signedIn, center, connection, state, t, toast]);
}
