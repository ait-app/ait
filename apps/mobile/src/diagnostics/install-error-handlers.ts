import { appErrorReporter, errorMessage } from "./error-reporter";

type NativeErrorHandler = (error: Error, isFatal?: boolean) => void;
interface NativeErrorUtils {
  getGlobalHandler: () => NativeErrorHandler;
  setGlobalHandler: (handler: NativeErrorHandler) => void;
}

/** Preserve platform handlers while collecting console failures and unhandled exceptions. */
export function installErrorHandlers(): () => void {
  const originalError = console.error;
  const originalWarn = console.warn;
  let collecting = false;
  const capture = (level: "warn" | "error", args: unknown[]) => {
    if (collecting) return;
    collecting = true;
    try {
      appErrorReporter.log(level, args);
      let exception: unknown = args.find((value) => value instanceof Error);
      let fields: unknown;
      if (!exception) {
        for (const value of args) {
          if (typeof value !== "string" || !value.startsWith("{")) continue;
          try {
            const parsed = JSON.parse(value);
            if (parsed && typeof parsed === "object" && parsed.err) {
              exception = parsed.err;
              fields = parsed;
              break;
            }
          } catch {
            /* Truncated console records remain available in the log tail. */
          }
        }
      }
      if (
        level === "error" ||
        exception ||
        /unhandled.*(?:rejection|promise)/i.test(String(args[0]))
      ) {
        // Console records are background diagnostics: keep them for export, but only
        // uncaught failures and explicit UI errors interrupt the user with a toast.
        appErrorReporter.report(exception ?? args, {
          source: `console.${level}`,
          notify: false,
          message: exception ? errorMessage(exception) : args.map(errorMessage).join(" "),
          context: fields ?? { logLabel: typeof args[0] === "string" ? args[0] : undefined },
        });
      }
    } catch {
      // Reporting must never prevent the original platform handler from running.
    } finally {
      collecting = false;
    }
  };
  const errorHandler: typeof console.error = (...args) => {
    capture("error", args);
    originalError.apply(console, args);
  };
  const warnHandler: typeof console.warn = (...args) => {
    capture("warn", args);
    originalWarn.apply(console, args);
  };
  console.error = errorHandler;
  console.warn = warnHandler;

  const onError = (event: ErrorEvent) => {
    // Resource loading events have no exception. Their callers report actionable failures.
    if (!event.message && !event.error) return;
    appErrorReporter.report(event.error ?? event.message, {
      source: "uncaught exception",
      context: { filename: event.filename, line: event.lineno, column: event.colno },
    });
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    appErrorReporter.report(event.reason, { source: "unhandled promise rejection" });
  };
  if (typeof window !== "undefined") {
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
  }
  const nativeUtils = (globalThis as typeof globalThis & { ErrorUtils?: NativeErrorUtils })
    .ErrorUtils;
  const previousNativeHandler = nativeUtils?.getGlobalHandler();
  const nativeHandler: NativeErrorHandler = (error, isFatal) => {
    appErrorReporter.report(error, { source: "native exception", context: { isFatal } });
    previousNativeHandler?.(error, isFatal);
  };
  nativeUtils?.setGlobalHandler(nativeHandler);

  return () => {
    if (console.error === errorHandler) console.error = originalError;
    if (console.warn === warnHandler) console.warn = originalWarn;
    if (typeof window !== "undefined") {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    }
    if (previousNativeHandler && nativeUtils?.getGlobalHandler() === nativeHandler) {
      nativeUtils.setGlobalHandler(previousNativeHandler);
    }
  };
}
