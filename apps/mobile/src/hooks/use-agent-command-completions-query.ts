import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useRetainedPanelActive } from "@/components/retained-panel";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import type { DraftCommandConfig } from "./use-agent-commands-query";

/** Native completion is keyed by the entire editor state; older edits never become current. */
export function useAgentCommandCompletionsQuery(input: {
  serverId: string;
  agentId: string;
  draftConfig?: DraftCommandConfig;
  text: string;
  cursor: number;
  enabled: boolean;
  revision: number;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(input.serverId);
  const connected = useHostRuntimeIsConnected(input.serverId);
  const panelActive = useRetainedPanelActive();
  const supported = useSessionStore(
    (state) => state.sessions[input.serverId]?.serverInfo?.features?.agentCommandCompletion,
  );
  const active = input.enabled && !!client && connected && panelActive && supported === true;
  const [settled, setSettled] = useState({
    text: input.text,
    cursor: input.cursor,
  });
  useEffect(() => {
    const timer = setTimeout(() => setSettled({ text: input.text, cursor: input.cursor }), 120);
    return () => clearTimeout(timer);
  }, [input.text, input.cursor]);
  const current = settled.text === input.text && settled.cursor === input.cursor;
  const query = useQuery({
    queryKey: [
      "agentNativeCompletions",
      input.serverId,
      input.agentId,
      input.draftConfig,
      input.text,
      input.cursor,
      input.revision,
    ],
    queryFn: async () => {
      if (!client) throw new Error(t("common.errors.daemonClientUnavailable"));
      try {
        return await client.completeCommands({
          agentId: input.agentId,
          draftConfig: input.draftConfig,
          text: input.text,
          cursor: input.cursor,
        });
      } catch (error) {
        // A missing method is compatibility fallback; native execution failures remain errors.
        if (error instanceof Error && "code" in error && error.code === "not_implemented") {
          return { supported: false, items: [] };
        }
        throw error;
      }
    },
    enabled: active && current,
    retry: false,
    staleTime: 0,
    gcTime: 0,
  });
  return {
    active,
    result: current ? query.data : undefined,
    isLoading: active && (!current || query.isPending),
    error: query.error,
  };
}
