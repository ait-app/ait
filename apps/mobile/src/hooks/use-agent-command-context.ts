import { useShallow } from "zustand/react/shallow";
import { useSessionStore } from "@/stores/session-store";

/** Separate catalogs/completions after a provider, project or native session changes. */
export function useAgentCommandContext(serverId: string, agentId: string) {
  return useSessionStore(
    useShallow((state) => {
      const agent = state.sessions[serverId]?.agents?.get(agentId);
      return agent
        ? {
            provider: agent.provider,
            cwd: agent.cwd ?? "",
            nativeSessionId: agent.runtimeInfo?.sessionId ?? agent.persistence?.sessionId ?? null,
          }
        : undefined;
    }),
  );
}
