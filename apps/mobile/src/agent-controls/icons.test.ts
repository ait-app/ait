import { describe, expect, it, vi } from "vitest";
import { Bot, Hammer } from "lucide-react-native";
import type { AgentProviderDefinition } from "@ait/protocol/provider-manifest";
import { getAgentModeIcon } from "./icons";

vi.mock("lucide-react-native", () =>
  Object.fromEntries(
    [
      "Bot",
      "Brain",
      "Hammer",
      "ListTodo",
      "Settings2",
      "Shield",
      "ShieldAlert",
      "ShieldCheck",
      "ShieldEllipsis",
      "ShieldOff",
      "ShieldPlus",
      "ShieldQuestionMark",
      "Zap",
    ].map((name) => [name, () => null]),
  ),
);

const definitions: AgentProviderDefinition[] = [
  {
    id: "opencode",
    label: "OpenCode",
    description: "",
    defaultModeId: "build",
    modes: [{ id: "build", label: "Build", icon: "Hammer", colorTier: "moderate" }],
  },
];

describe("OpenCode control icons", () => {
  it("shows the declared hammer in both the selected control and mode menu", () => {
    expect(getAgentModeIcon("opencode", "build", definitions)).toBe(Hammer);
  });
  it("retains the unknown-mode fallback", () => {
    expect(getAgentModeIcon("opencode", "unknown", definitions)).toBe(Bot);
  });
});
