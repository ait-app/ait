import { buildAgentMessageDisplay } from "@ait/protocol/paseo-tool-presentation/index";
import type { AgentMessage } from "@ait/protocol/agent-message";
import type { PlanOutcome } from "@/components/plan-card";
import type { ComponentType } from "react";
import type { ToolCallDetail } from "@ait/protocol/agent-types";
import type { ToolCallDisplayInput } from "@/utils/tool-call-display";
import { buildToolCallDisplayModel } from "@/utils/tool-call-display";
import { extractToolCallFilePath } from "@/utils/extract-tool-call-file-path";
import {
  hasMeaningfulToolCallDetail,
  isPendingToolCallDetail,
} from "@/utils/tool-call-detail-state";

type ToolCallStatus = "executing" | "running" | "completed" | "failed" | "canceled";
export type ToolCallPresentationIcon = ComponentType<{ size?: number; color?: string }>;

interface BuildToolCallPresentationInput {
  agentMessage?: AgentMessage;
  toolName: string;
  status: ToolCallStatus;
  error: unknown;
  detail?: ToolCallDetail;
  cwd?: string;
  metadata?: Record<string, unknown>;
  targetAgentTitle?: string;
  resolveIcon: ToolCallIconResolver;
}

export interface ToolCallPresentation {
  detail?: ToolCallDetail;
  displayName: string;
  summary?: string;
  errorText?: string;
  icon: ToolCallPresentationIcon;
  isLoadingDetails: boolean;
  hasDetails: boolean;
  canOpenDetails: boolean;
  openFilePath: string | null;
  isPlan: boolean;
  planOutcome?: PlanOutcome;
}

export type ToolCallIconResolver = (
  toolName: string,
  detail: ToolCallDetail | undefined,
  agentMessage: AgentMessage | undefined,
) => ToolCallPresentationIcon;

function displayStatus(status: ToolCallStatus): ToolCallDisplayInput["status"] {
  return status === "executing" ? "running" : status;
}

function displayDetail(detail: ToolCallDetail | undefined): ToolCallDetail {
  return detail ?? { type: "unknown", input: null, output: null };
}

export function buildToolCallPresentation(
  input: BuildToolCallPresentationInput,
): ToolCallPresentation {
  const detail = input.agentMessage
    ? buildAgentMessageDisplay(input.agentMessage).detail
    : input.detail;
  const detailForDisplay = displayDetail(detail);
  const displayModel = buildToolCallDisplayModel({
    name: input.toolName,
    agentMessage: input.agentMessage,
    status: displayStatus(input.status),
    error: input.error ?? null,
    detail: detailForDisplay,
    metadata: input.metadata,
    cwd: input.cwd,
    targetAgentTitle: input.targetAgentTitle,
  });
  const isLoadingDetails = isPendingToolCallDetail({
    detail,
    status: input.status,
    error: input.error,
    toolName: input.toolName,
  });
  const hasDetails =
    Boolean(displayModel.errorText?.trim()) || hasMeaningfulToolCallDetail(detail, input.toolName);

  return {
    detail,
    displayName: displayModel.displayName,
    summary: displayModel.summary,
    errorText: displayModel.errorText,
    icon: input.resolveIcon(input.toolName, detail, input.agentMessage),
    isLoadingDetails,
    hasDetails,
    canOpenDetails: hasDetails,
    openFilePath: extractToolCallFilePath(input.detail),
    isPlan: input.detail?.type === "plan",
    planOutcome: input.detail?.type === "plan" ? resolvePlanOutcome(input) : undefined,
  };
}

function resolvePlanOutcome(input: BuildToolCallPresentationInput): PlanOutcome | undefined {
  if (input.status === "canceled") return "canceled";
  if (input.metadata?.approved === false) return "rejected";
  if (input.metadata?.approved === true) return "approved";
  if (input.status === "running" || input.status === "executing") return "pending";
  return undefined;
}
