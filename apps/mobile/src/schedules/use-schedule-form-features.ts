import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useFetchQuery } from "@/data/query";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import type { ScheduleFormModel, ScheduleFormState } from "./schedule-form-model";

/**
 * Lists the selected provider's features (such as a permission mode) in the
 * schedule's directory on the schedule's host, and hands them to the form.
 */
export function useScheduleFormFeatures(model: ScheduleFormModel, state: ScheduleFormState): void {
  const { t } = useTranslation();
  const request = state.featureRequest;
  const requestKey = state.featureRequestKey;
  const serverId = request?.serverId ?? "";
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);

  // `dataShape: "value"` and not `"list"`: a list shape keeps the previous
  // data while the next request is in flight, which would hand the features
  // of one provider/model combination to the key of another.
  const featuresQuery = useFetchQuery({
    queryKey: ["scheduleFormFeatures", requestKey],
    dataShape: "value",
    staleTimeMs: 5 * 60 * 1000,
    enabled: Boolean(client && isConnected && request),
    retry: false,
    queryFn: async () => {
      if (!client || !request) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      const payload = await client.listProviderFeatures({
        provider: request.provider,
        cwd: request.cwd,
        ...(request.model ? { model: request.model } : {}),
        ...(request.modeId ? { modeId: request.modeId } : {}),
        ...(request.thinkingOptionId ? { thinkingOptionId: request.thinkingOptionId } : {}),
      });
      if (payload.error) {
        throw new Error(payload.error);
      }
      return payload.features ?? [];
    },
  });

  const { data, isError } = featuresQuery;

  useEffect(() => {
    if (!requestKey || !data) {
      return;
    }
    model.applyFeatures(requestKey, data);
  }, [data, model, requestKey]);

  useEffect(() => {
    if (!requestKey || !isError) {
      return;
    }
    model.applyFeaturesUnavailable(requestKey);
  }, [isError, model, requestKey]);
}
