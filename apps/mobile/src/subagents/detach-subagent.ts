import type { Agent } from "@/stores/session-store";
import { resolveAgentTitleLabel } from "@/utils/agent-title-label";
import type { ConfirmDialogInput } from "@/utils/confirm-dialog";

export interface ResolveDetachSubagentDialogInput {
  title: Agent["title"] | null | undefined;
}

export function resolveDetachSubagentDialog(
  input: ResolveDetachSubagentDialogInput,
): ConfirmDialogInput {
  const subagentLabel = resolveAgentTitleLabel(input.title) ?? "This subagent";

  return {
    title: "Detach subagent?",
    message: `${subagentLabel} will leave this track and continue as a standalone agent.`,
    confirmLabel: "Detach",
    cancelLabel: "Cancel",
    destructive: false,
  };
}

export interface DetachSubagentDeps {
  getSubagent: (subagentId: string) => ResolveDetachSubagentDialogInput | undefined;
  confirm: (input: ConfirmDialogInput) => Promise<boolean>;
  detachAgent: (input: { serverId: string; agentId: string }) => Promise<void>;
  openDetachedAgent: (input: { serverId: string; agentId: string }) => void;
  reportError: (error: unknown) => void;
}

export interface RequestDetachSubagentInput {
  serverId: string;
  subagentId: string;
}

export async function requestDetachSubagent(
  input: RequestDetachSubagentInput,
  deps: DetachSubagentDeps,
): Promise<void> {
  const subagent = deps.getSubagent(input.subagentId);
  const confirmed = await deps.confirm(
    resolveDetachSubagentDialog({
      title: subagent?.title,
    }),
  );
  if (!confirmed) {
    return;
  }
  try {
    await deps.detachAgent({ serverId: input.serverId, agentId: input.subagentId });
    deps.openDetachedAgent({ serverId: input.serverId, agentId: input.subagentId });
  } catch (error) {
    deps.reportError(error);
  }
}
