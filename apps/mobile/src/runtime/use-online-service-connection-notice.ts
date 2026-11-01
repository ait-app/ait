import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useToast } from "@/contexts/toast-api-context";
import { useAccountState } from "./account-state";
import { useHubServices } from "./hub-service-store";

/** Keep recovery neutral; report unavailability once without repeating on retries. */
export function useOnlineServiceConnectionNotice(): void {
  const { t } = useTranslation();
  const toast = useToast();
  const signedIn = useAccountState((state) => state.status !== "logged_out");
  const center = useAccountState((state) => state.center);
  const connection = useHubServices((state) => state.connection);
  const state = useHubServices((state) => state.snapshot?.state);
  const notice = useRef({ center, connection, loadingShown: false, errorShown: false });

  useEffect(() => {
    if (notice.current.center !== center || notice.current.connection !== connection) {
      notice.current = { center, connection, loadingShown: false, errorShown: false };
    }
    if (!signedIn || !state || state === "online" || state === "stopped") {
      notice.current.loadingShown = false;
      notice.current.errorShown = false;
      return;
    }
    if (state === "reconnecting" && !notice.current.loadingShown && !notice.current.errorShown) {
      notice.current.loadingShown = true;
      toast.show(t("onlineService.reconnecting"), { variant: "loading" });
    } else if (state === "unavailable" && !notice.current.errorShown) {
      notice.current.errorShown = true;
      toast.error(t("onlineService.disconnected"));
    }
  }, [signedIn, center, connection, state, t, toast]);
}
