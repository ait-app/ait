import type { ToolCallDetail } from "@ait/protocol/agent-types";
import { buildPaseoToolDetailSections } from "@ait/protocol/paseo-tool-call-detail";
import { parseUnifiedDiff } from "./tool-call-parsers";

function hasText(value: string | undefined): boolean {
  return Boolean(value?.trim());
}

function hasMeaningfulUnknownValue(value: unknown): boolean {
  if (value === null || value === undefined) {
    return false;
  }
  if (typeof value === "string") {
    return value.trim().length > 0;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return true;
  }
  if (Array.isArray(value)) {
    return value.some(hasMeaningfulUnknownValue);
  }
  if (typeof value === "object") {
    return Object.values(value).some(hasMeaningfulUnknownValue);
  }
  return true;
}

function hasMeaningfulSearchDetail(detail: Extract<ToolCallDetail, { type: "search" }>): boolean {
  return Boolean(
    hasText(detail.content) ||
    detail.filePaths?.some(hasText) ||
    detail.webResults?.some((result) => hasText(result.title) || hasText(result.url)) ||
    detail.annotations?.some(hasText),
  );
}

function hasMeaningfulEditDetail(detail: Extract<ToolCallDetail, { type: "edit" }>): boolean {
  return detail.unifiedDiff
    ? parseUnifiedDiff(detail.unifiedDiff).some((line) => hasText(line.content))
    : Boolean(detail.oldString || detail.newString);
}

function hasMeaningfulFetchDetail(detail: Extract<ToolCallDetail, { type: "fetch" }>): boolean {
  return hasText(detail.url) || hasText(detail.result);
}

function hasMeaningfulWorktreeSetupDetail(
  detail: Extract<ToolCallDetail, { type: "worktree_setup" }>,
): boolean {
  return hasText(detail.branchName) || hasText(detail.worktreePath) || hasText(detail.log);
}

function hasMeaningfulSubAgentDetail(
  detail: Extract<ToolCallDetail, { type: "sub_agent" }>,
): boolean {
  return (
    hasText(detail.subAgentType) ||
    hasText(detail.description) ||
    hasText(detail.childSessionId) ||
    hasText(detail.log)
  );
}

function hasMeaningfulUnknownDetail(
  detail: Extract<ToolCallDetail, { type: "unknown" }>,
  toolName: string | undefined,
): boolean {
  const sections = toolName
    ? buildPaseoToolDetailSections(toolName, detail.input, detail.output)
    : null;
  if (sections) {
    return sections.some((section) =>
      section.kind === "prose"
        ? hasText(section.text)
        : section.fields.some((field) => hasText(field.value)),
    );
  }
  return hasMeaningfulUnknownValue(detail.input) || hasMeaningfulUnknownValue(detail.output);
}

export function hasMeaningfulToolCallDetail(
  detail: ToolCallDetail | undefined,
  toolName?: string,
): boolean {
  if (!detail) {
    return false;
  }

  switch (detail.type) {
    case "shell":
      return hasText(detail.command) || hasText(detail.output);
    case "read":
    case "write":
      return hasText(detail.content);
    case "edit":
      return hasMeaningfulEditDetail(detail);
    case "search":
      return hasMeaningfulSearchDetail(detail);
    case "fetch":
      return hasMeaningfulFetchDetail(detail);
    case "worktree_setup":
      return hasMeaningfulWorktreeSetupDetail(detail);
    case "sub_agent":
      return hasMeaningfulSubAgentDetail(detail);
    case "plain_text":
      return hasText(detail.text);
    case "plan":
      return detail.text.trim().length > 0;
    case "unknown":
      return hasMeaningfulUnknownDetail(detail, toolName);
  }
}

export function isPendingToolCallDetail(params: {
  detail: ToolCallDetail | undefined;
  status: "executing" | "running" | "completed" | "failed" | "canceled";
  error: unknown;
  toolName?: string;
}): boolean {
  const isRunning = params.status === "running" || params.status === "executing";
  return (
    isRunning &&
    params.error == null &&
    !hasMeaningfulToolCallDetail(params.detail, params.toolName)
  );
}
