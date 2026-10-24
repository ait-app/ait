import { describe, expect, it, vi } from "vitest";
import { Bot, Hammer, Minimize2, RotateCcw, Settings2, Turtle, Zap } from "lucide-react-native";
import type { AgentProviderDefinition } from "@ait/protocol/provider-manifest";
import { getAgentFeatureIcon, getAgentFeatureValueIcon, getAgentModeIcon } from "./icons";

vi.mock("lucide-react-native", () =>
  Object.fromEntries(
    [
      "Bot",
      "Brain",
      "Hammer",
      "ListTodo",
      "Minimize2",
      "RotateCcw",
      "Settings2",
      "Shield",
      "ShieldAlert",
      "ShieldCheck",
      "ShieldEllipsis",
      "ShieldOff",
      "ShieldPlus",
      "ShieldQuestionMark",
      "Zap",
      "Turtle",
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

describe("OMP semantic icons", () => {
  it("uses distinct semantic icons when native feature metadata omits the icon", () => {
    for (const [id, icon] of [
      ["autoCompaction", Minimize2],
      ["autoRetry", RotateCcw],
      ["fastMode", Zap],
      ["slowMode", Turtle],
    ] as const) {
      expect(getAgentFeatureIcon(undefined, id)).toBe(icon);
      expect(getAgentFeatureValueIcon({ id }, false)).toBe(icon);
    }
  });
  it("respects a known declared icon and retains the unknown control fallback", () => {
    expect(getAgentFeatureIcon("zap", "autoRetry")).toBe(Zap);
    expect(getAgentFeatureIcon(undefined, "custom")).toBe(Settings2);
    expect(getAgentFeatureIcon("unknown", "custom")).toBe(Settings2);
  });
});
