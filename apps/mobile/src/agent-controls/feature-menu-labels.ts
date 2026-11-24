import type { TFunction } from "i18next";
import type { AgentFeature } from "@ait/protocol/agent-types";
import type { AgentAutocompleteOption } from "@/hooks/use-agent-autocomplete";
import { formatAgentFeatureLabel } from "./labels";

const COMMAND_KEYS: Record<string, string> = {
  advisor: "advisor",
  btw: "btw",
  compact: "compact",
  autocompact: "autoCompaction",
  goal: "goal",
  "guided-goal": "guidedGoal",
  vibe: "vibe",
  handoff: "handoff",
  steer: "steer",
  "follow-up": "followUp",
  help: "help",
  status: "status",
  usage: "usage",
  stats: "stats",
  session: "session",
  trace: "trace",
  tools: "tools",
  context: "context",
  jobs: "jobs",
  dirs: "dirs",
  changelog: "changelog",
  memory: "memory",
  security: "security",
  extensions: "extensions",
  plan: "plan",
  loop: "loop",
  queue: "queue",
  tan: "tan",
  omfg: "omfg",
  cleanse: "cleanse",
  ratchet: "ratchet",
  force: "force",
};
const OMP_FALLBACK_COMMANDS = new Set(["compact", "autocompact", "handoff", "steer", "follow-up"]);
const OMP_FEATURE_COMMANDS: Record<string, string> = {
  advisorEnabled: "advisor",
  fastMode: "fast",
  slowMode: "slow",
  autoCompaction: "autocompact",
};

export function presentFeature(feature: AgentFeature, provider: string | undefined, t: TFunction) {
  const command = provider === "omp" ? OMP_FEATURE_COMMANDS[feature.id] : undefined;
  return {
    label: formatAgentFeatureLabel(feature, t),
    description: command ? `/${command}` : undefined,
  };
}

/** Native names remain execution identities; only builtin-owned rows receive builtin translations. */
export function presentFeatureCommand(
  option: AgentAutocompleteOption,
  provider: string | undefined,
  t: TFunction,
) {
  if (option.type === "workspace_entry") return null;
  const command = option.command;
  const name = command.name;
  let key: string | undefined;
  if (option.type === "client_command") {
    key = option.command.kind === "archive-agent" ? "archive" : "newConversation";
  } else if (option.command.source === "ait" && name === "btw") {
    key = "btw";
  } else if (
    option.command.kind !== "skill" &&
    (option.command.source === "builtin" ||
      (!option.command.source &&
        (provider === "codex" ||
          provider === "deepseek-harness" ||
          (provider === "omp" && OMP_FALLBACK_COMMANDS.has(name)))))
  ) {
    key = COMMAND_KEYS[name];
  }
  return {
    label: key ? t(`agentControls.commands.${key}`) : name,
    description: `/${name}`,
    searchTerms: [name, ...(command.aliases ?? []).map((alias) => `/${alias.replace(/:+$/u, "")}`)],
  };
}
