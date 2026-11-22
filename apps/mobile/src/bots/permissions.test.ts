import { describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { botSessionNotes, defaultAgentMode, defaultBotPermissions } from "./permissions";

const claude: ProviderSnapshotEntry = {
  provider: "claude",
  status: "ready",
  enabled: true,
  defaultModeId: "default",
  modes: [
    { id: "plan", label: "Plan", colorTier: "planning" },
    { id: "default", label: "Ask", colorTier: "safe" },
    { id: "acceptEdits", label: "Edits", colorTier: "moderate" },
    { id: "bypassPermissions", label: "Bypass", colorTier: "dangerous" },
  ],
};
const codex: ProviderSnapshotEntry = {
  provider: "codex",
  status: "ready",
  enabled: true,
  defaultModeId: "auto",
  modes: [
    { id: "auto", label: "Default", colorTier: "moderate" },
    { id: "full-access", label: "Full", colorTier: "dangerous" },
  ],
};

describe("bot permissions defaults", () => {
  it("selects the provider's default attended mode", () => {
    expect(defaultAgentMode(claude)).toBe("default");
    expect(defaultAgentMode(codex)).toBe("auto");
  });

  it("falls back to the first attended mode without a default", () => {
    expect(defaultAgentMode({ ...codex, defaultModeId: null })).toBe("auto");
    expect(defaultAgentMode({ ...codex, modes: [] })).toBeUndefined();
  });

  it("builds default groups and omits providers that are not ready", () => {
    const profile = defaultBotPermissions([
      claude,
      codex,
      { ...codex, provider: "opencode", status: "unavailable" },
    ]);
    expect(profile.groups).toEqual(["observe", "agents", "stop", "schedules"]);
    expect(profile.modes).toEqual({ claude: ["default"], codex: ["auto"] });
  });

  it("bot_session_notes_describe_degraded_capabilities", () => {
    expect(
      botSessionNotes({
        instructions: "system_prompt",
        readOnlyMode: "read-only",
        toolPreapproval: true,
        aitTools: true,
      }),
    ).toEqual([]);
    expect(botSessionNotes(undefined)).toEqual([]);
    expect(
      botSessionNotes({
        instructions: "turn_prefix",
        readOnlyMode: null,
        toolPreapproval: false,
        aitTools: true,
      }),
    ).toEqual(["instructionsInInput", "noReadOnlyMode", "toolsNeedApproval"]);
    const none = botSessionNotes({
      instructions: "turn_prefix",
      readOnlyMode: null,
      toolPreapproval: false,
      aitTools: false,
    });
    expect(none).toContain("noAitTools");
    expect(none).not.toContain("toolsNeedApproval");
  });
});
