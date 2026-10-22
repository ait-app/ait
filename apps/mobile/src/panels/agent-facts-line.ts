import type { AgentMode, AgentModelDefinition } from "@ait/protocol/agent-types";
import { useShallow } from "zustand/shallow";
import { formatAgentModeLabel } from "@/agent-controls/labels";
import { formatTokenCount } from "@/components/context-window-meter.utils";
import {
  getRuntimeThinkingState,
  resolveAgentModelSelection,
  type RuntimeThinkingState,
} from "@/composer/agent-controls/utils";
import { useProvidersSnapshot } from "@/hooks/use-providers-snapshot";
import { useSessionStore } from "@/stores/session-store";

export interface AgentFactsInput {
  models: AgentModelDefinition[] | null;
  runtimeModelId: string | null | undefined;
  configuredModelId: string | null | undefined;
  runtimeThinkingState: RuntimeThinkingState;
  explicitThinkingOptionId: string | null | undefined;
  availableModes: AgentMode[];
  currentModeId: string | null | undefined;
  contextWindowUsedTokens: number | null | undefined;
  contextWindowMaxTokens: number | null | undefined;
}

/**
 * Model, effective thinking, mode, and context usage as one line. Pi's native thinking may
 * differ from the requested selection shown by the composer's control. Null until the agent
 * has a model at all.
 */
export function buildAgentFactsLine(input: AgentFactsInput): string | null {
  const hasModel = Boolean(input.runtimeModelId?.trim() || input.configuredModelId?.trim());
  if (!hasModel) return null;
  const selection = resolveAgentModelSelection(input);
  const parts = [selection.displayModel];
  if (selection.displayThinkingId || selection.isThinkingUnknown) {
    parts.push(selection.displayThinking);
  }
  const mode = resolveModeLabel(input.availableModes, input.currentModeId);
  if (mode) parts.push(mode);
  const context = formatContextUsage(input.contextWindowUsedTokens, input.contextWindowMaxTokens);
  if (context) parts.push(context);
  return parts.join(" · ");
}

/** The facts line for a Paseo agent in the session store, kept current as the agent reports. */
export function useAgentFactsLine(serverId: string, agentId: string): string | null {
  const agent = useSessionStore(
    useShallow((state) => {
      const current = state.sessions[serverId]?.agents.get(agentId);
      return current
        ? {
            provider: current.provider,
            cwd: current.cwd,
            runtimeModelId: current.runtimeInfo?.model ?? null,
            configuredModelId: current.model ?? null,
            runtimeInfo: current.runtimeInfo,
            explicitThinkingOptionId: current.thinkingOptionId ?? null,
            availableModes: current.availableModes,
            currentModeId: current.currentModeId,
            contextWindowUsedTokens: current.lastUsage?.contextWindowUsedTokens ?? null,
            contextWindowMaxTokens: current.lastUsage?.contextWindowMaxTokens ?? null,
          }
        : null;
    }),
  );
  const { entries } = useProvidersSnapshot(serverId, { cwd: agent?.cwd });
  const models = entries?.find((entry) => entry.provider === agent?.provider)?.models ?? null;
  return agent
    ? buildAgentFactsLine({
        ...agent,
        models,
        runtimeThinkingState: getRuntimeThinkingState(agent.runtimeInfo),
      })
    : null;
}

/** The mode control shows the current mode, or the provider's first one until the agent reports. */
function resolveModeLabel(modes: AgentMode[], currentModeId: string | null | undefined) {
  const mode = modes.find((candidate) => candidate.id === currentModeId) ?? modes[0];
  return mode ? formatAgentModeLabel(mode) : null;
}

/** The context meter's numbers: used of max, with the share it draws as the ring. */
function formatContextUsage(
  usedTokens: number | null | undefined,
  maxTokens: number | null | undefined,
): string | null {
  if (
    typeof usedTokens !== "number" ||
    typeof maxTokens !== "number" ||
    !Number.isFinite(usedTokens) ||
    !Number.isFinite(maxTokens) ||
    usedTokens < 0 ||
    maxTokens <= 0
  ) {
    return null;
  }
  const percentage = Math.round(Math.min(100, (usedTokens / maxTokens) * 100));
  return `${formatTokenCount(usedTokens)} / ${formatTokenCount(maxTokens)} tokens (${percentage}%)`;
}
