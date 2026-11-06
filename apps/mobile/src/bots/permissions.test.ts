import { describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { botSessionNotes, defaultBotPermissions, defaultModeCap } from "./permissions";

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
  it("default_mode_cap_allows_default_and_safer_modes", () => {
    expect(defaultModeCap(claude)).toEqual(["default", "plan"]);
    expect(defaultModeCap(codex)).toEqual(["auto"]);
  });

  it("falls back to the first attended mode without a default", () => {
    expect(defaultModeCap({ ...codex, defaultModeId: null })).toEqual(["auto"]);
    expect(defaultModeCap({ ...codex, modes: [] })).toEqual([]);
  });

  it("builds default groups and omits providers that are not ready", () => {
    const profile = defaultBotPermissions([
      claude,
      codex,
      { ...codex, provider: "opencode", status: "unavailable" },
    ]);
    expect(profile.groups).toEqual(["observe", "agents", "stop", "schedules"]);
    expect(profile.modes).toEqual({ claude: ["default", "plan"], codex: ["auto"] });
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
