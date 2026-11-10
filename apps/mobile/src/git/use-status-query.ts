import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { checkoutStatusQueryKey } from "@/git/query-keys";
import { fetchCheckoutStatus } from "./checkout-status-cache";

export type { CheckoutStatusPayload } from "./checkout-status-cache";

interface UseCheckoutStatusQueryOptions {
  serverId: string;
  cwd: string;
}

export function useCheckoutStatusQuery({ serverId, cwd }: UseCheckoutStatusQueryOptions) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const isConnected = useHostRuntimeIsConnected(serverId);

  const query = useQuery({
    queryKey: checkoutStatusQueryKey(serverId, cwd),
    queryFn: async () => {
      if (!client) {
        throw new Error(t("common.errors.daemonClientUnavailable"));
      }
      return await fetchCheckoutStatus({ client, serverId, cwd });
    },
    enabled: !!client && isConnected && !!cwd,
    staleTime: Infinity,
    // Freshness is push-driven (checkout.status.update applied globally); with
    // staleTime: Infinity, refetchOnMount only fires after an explicit invalidation
    // (e.g. reconnect), which is exactly when the push stream may have been missed.
    refetchOnMount: true,
    refetchOnReconnect: false,
    refetchOnWindowFocus: false,
  });

  return {
    status: query.data ?? null,
    isLoading: query.isLoading,
    isFetching: query.isFetching,
    isError: query.isError,
    error: query.error,
  };
}
