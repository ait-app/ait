import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useToast } from "@/contexts/toast-api-context";
import { useAccountState } from "./account-state";
import { useHubServices } from "./hub-service-store";

/** Report each outage once; retry attempts must not keep interrupting the user. */
export function useOnlineServiceConnectionNotice(): void {
  const { t } = useTranslation();
  const toast = useToast();
  const signedIn = useAccountState((state) => state.status !== "logged_out");
  const center = useAccountState((state) => state.center);
  const connection = useHubServices((state) => state.connection);
  const state = useHubServices((state) => state.snapshot?.state);
  const notice = useRef({ center, connection, notified: false });

  useEffect(() => {
    if (notice.current.center !== center || notice.current.connection !== connection) {
      notice.current = { center, connection, notified: false };
    }
    if (!signedIn || !state || state === "online" || state === "stopped") {
      notice.current.notified = false;
      return;
    }
    if (!notice.current.notified && (state === "reconnecting" || state === "unavailable")) {
      notice.current.notified = true;
      toast.error(t("onlineService.disconnected"));
    }
  }, [signedIn, center, connection, state, t, toast]);
}
