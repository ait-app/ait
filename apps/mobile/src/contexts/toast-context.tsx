import { useEffect, type ReactNode } from "react";
import { appErrorReporter } from "@/diagnostics/error-reporter";
import { ToastViewport, useToastHost } from "@/components/toast-host";
import { ToastApiProvider } from "./toast-api-context";

export { ToastApiProvider, useToast } from "./toast-api-context";

export function ToastProvider({ children }: { children: ReactNode }) {
  const { api, toast, dismiss } = useToastHost();

  useEffect(() => {
    const flush = () => {
      for (const report of appErrorReporter.takePending()) {
        api.show(report.message, { variant: "error", errorReport: report });
      }
    };
    const unsubscribe = appErrorReporter.subscribe(flush);
    flush();
    return unsubscribe;
  }, [api]);

  return (
    <ToastApiProvider api={api}>
      {children}
      <ToastViewport toast={toast} onDismiss={dismiss} />
    </ToastApiProvider>
  );
}
