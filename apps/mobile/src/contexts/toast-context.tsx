import { useEffect, type ReactNode } from "react";
import { ToastViewport, useToastHost } from "@/components/toast-host";
import { formatAlertNotice, setAlertPresenter } from "@/utils/show-alert";
import { ToastApiProvider } from "./toast-api-context";

export { ToastApiProvider, useToast } from "./toast-api-context";

// An alert carries a title and a detail line, so it stays up longer than a one-line toast.
const ALERT_TOAST_DURATION_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const { api, toast, dismiss } = useToastHost();

  useEffect(
    () =>
      setAlertPresenter((notice) => {
        api.show(formatAlertNotice(notice), {
          variant: "error",
          durationMs: ALERT_TOAST_DURATION_MS,
        });
      }),
    [api],
  );

  return (
    <ToastApiProvider api={api}>
      {children}
      <ToastViewport toast={toast} onDismiss={dismiss} />
    </ToastApiProvider>
  );
}
