import { useCallback, useMemo, useRef, useState } from "react";
import { AgentConfigUncertainError } from "@ait/client/internal/daemon-client";
import type { TFunction } from "i18next";
import type { ToastApi } from "@/components/toast-host";
import { toErrorMessage } from "@/utils/error-messages";

export interface ConfigMutationGate {
  busy: boolean;
  run: (task: () => Promise<void>) => Promise<void>;
}

/** Serialize configuration gestures across every control in the same composer. */
export function useConfigMutationGate(): ConfigMutationGate {
  const active = useRef(false);
  const [busy, setBusy] = useState(false);
  const run = useCallback(async (task: () => Promise<void>) => {
    if (active.current) return;
    active.current = true;
    setBusy(true);
    try {
      await task();
    } finally {
      active.current = false;
      setBusy(false);
    }
  }, []);
  return useMemo(() => ({ busy, run }), [busy, run]);
}

/** Refresh is only a current observation: an earlier queued request can still arrive later. */
export function showConfigRequestError(
  toast: ToastApi,
  t: TFunction,
  error: unknown,
  serverId: string,
): void {
  if (!(error instanceof AgentConfigUncertainError)) {
    toast.error(toErrorMessage(error));
    return;
  }
  toast.show(t("agentControls.configResultUnknown"), { variant: "warning", durationMs: 10000 });
  void import("@/runtime/host-runtime")
    .then(({ getHostRuntimeStore }) => getHostRuntimeStore().refreshAgentDirectory({ serverId }))
    .catch(() => undefined);
}
