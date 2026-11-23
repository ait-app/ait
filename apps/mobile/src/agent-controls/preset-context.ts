import { useQuery } from "@tanstack/react-query";
import { useSessionStore } from "@/stores/session-store";

export function useAgentPresetContext(input: {
  serverId: string | null;
  agentId?: string;
  revision?: number;
  enabled: boolean;
  poll?: boolean;
}) {
  const client = useSessionStore((state) => state.sessions[input.serverId ?? ""]?.client ?? null);
  return useQuery({
    queryKey: ["agent-model-preset-context", input.serverId, input.agentId, input.revision ?? 0],
    enabled: input.enabled && Boolean(client && input.agentId),
    retry: false,
    staleTime: 0,
    refetchInterval: input.poll ? 10_000 : false,
    refetchIntervalInBackground: false,
    queryFn: () => {
      if (!client || !input.agentId) throw new Error("Preset context unavailable");
      return client.getAgentModelPresetContext(input.agentId);
    },
  });
}
