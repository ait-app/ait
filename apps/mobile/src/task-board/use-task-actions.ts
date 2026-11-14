import { useIsMutating, useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useArchiveAgent } from "@/hooks/use-archive-agent";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { navigateToAgent } from "@/navigation/navigate-to-agent";
import { taskActions, type BoardTask } from "./model";

/** Cards and details share capabilities and pending state for the same agent. */
export function useTaskActions(task: BoardTask, online: boolean) {
  const { t } = useTranslation();
  const { agent } = task;
  const client = useHostRuntimeClient(agent.serverId);
  const { archiveAgent } = useArchiveAgent();
  const allowed = taskActions(task);
  const mutationKey = ["task-board-action", agent.serverId, agent.id];
  const pending = useIsMutating({ mutationKey });
  const action = useMutation({
    mutationKey,
    retry: false,
    networkMode: "always",
    mutationFn: async (kind: "stop" | "read" | "archive") => {
      if (!client || !online) throw new Error(t("taskBoard.offlineHint"));
      if (!allowed[kind]) throw new Error(t("taskBoard.actionFailed"));
      if (kind === "stop") await client.cancelAgent(agent.id);
      else if (kind === "read") await client.clearAgentAttention(agent.id);
      else await archiveAgent({ serverId: agent.serverId, agentId: agent.id });
    },
  });
  return {
    action,
    allowed,
    disabled: !online || !client || pending > 0,
    open: () =>
      navigateToAgent({
        serverId: agent.serverId,
        agentId: agent.id,
        workspaceId: agent.workspaceId,
        pin: true,
      }),
  };
}
