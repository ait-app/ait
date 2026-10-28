import type { AgentModelDefinition, AgentRuntimeInfo } from "@ait/protocol/agent-types";
import { normalizeAgentModelDefinition } from "@ait/protocol/agent-types";

export type RuntimeThinkingState =
  | { status: "unreported" }
  | { status: "reported"; optionId: string }
  | { status: "malformed" };

interface RuntimeControlPresentation {
  preferRuntimeModel: boolean;
  selectedThinkingId: string | null;
  effectiveThinking: RuntimeThinkingState;
}

interface ControlPresentationAdapter {
  preserveUnknownThinking: boolean;
  fallbackToFirstThinkingOption: boolean;
  readRuntime: (info: AgentRuntimeInfo | null | undefined) => RuntimeControlPresentation;
}

const REQUESTED_CONTROLS: RuntimeControlPresentation = {
  preferRuntimeModel: false,
  selectedThinkingId: null,
  effectiveThinking: { status: "unreported" },
};

const DEFAULT_ADAPTER: ControlPresentationAdapter = {
  preserveUnknownThinking: false,
  fallbackToFirstThinkingOption: true,
  readRuntime: () => REQUESTED_CONTROLS,
};

const PI_ADAPTER: ControlPresentationAdapter = {
  preserveUnknownThinking: true,
  fallbackToFirstThinkingOption: false,
  readRuntime(info) {
    const level = info?.thinkingOptionId ?? info?.extra?.thinkingLevel;
    const effectiveThinking: RuntimeThinkingState =
      level === undefined
        ? { status: "unreported" }
        : typeof level !== "string" || level.trim().length === 0
          ? { status: "malformed" }
          : { status: "reported", optionId: level };
    const pending = info?.extra?.configPending === true;
    return {
      preferRuntimeModel: !pending,
      selectedThinkingId:
        !pending && effectiveThinking.status === "reported" ? effectiveThinking.optionId : null,
      effectiveThinking,
    };
  },
};

function controlAdapter(provider: string | undefined): ControlPresentationAdapter {
  return provider === "pi" ? PI_ADAPTER : DEFAULT_ADAPTER;
}

/** Normalize provider runtime metadata before shared controls choose labels or selections. */
export function resolveProviderControlPresentation(
  provider?: string,
  runtimeInfo?: AgentRuntimeInfo | null,
) {
  const adapter = controlAdapter(provider ?? runtimeInfo?.provider);
  const { effectiveThinking, ...selection } = adapter.readRuntime(runtimeInfo);
  return {
    preserveUnknownThinking: adapter.preserveUnknownThinking,
    ...selection,
    runtimeThinkingStatus: effectiveThinking.status,
    runtimeThinkingOptionId:
      effectiveThinking.status === "reported" ? effectiveThinking.optionId : null,
  };
}

/** Read effective thinking independently of a queued or saved selection. */
export function getRuntimeThinkingState(
  info: AgentRuntimeInfo | null | undefined,
  provider?: string,
): RuntimeThinkingState {
  return controlAdapter(provider ?? info?.provider).readRuntime(info).effectiveThinking;
}

/** Resolve advertised defaults and the provider's policy for an unspecified selection. */
export function resolveDefaultThinkingOptionId(model: AgentModelDefinition | null): string {
  if (!model) return "";
  return (
    normalizeAgentModelDefinition(model).defaultThinkingOptionId ??
    (controlAdapter(model.provider).fallbackToFirstThinkingOption
      ? (model.thinkingOptions?.[0]?.id ?? "")
      : "")
  );
}
