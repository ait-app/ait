import { applyDaemonConfigPatch } from "@/data/daemon-config";
import type { MutableDaemonConfig } from "@ait/protocol/messages";
import { describe, expect, it } from "vitest";

const base: MutableDaemonConfig = {
  relay: { enabled: false },
  mcp: { injectIntoAgents: true },
  browserTools: { enabled: false },
  providers: {
    codex: { enabled: true, label: "Codex", env: { AIT: "1" }, params: { model: "gpt" } },
  },
  metadataGeneration: {
    providers: [{ provider: "codex", enabled: true }],
  },
  autoArchiveAfterMerge: false,
  enableTerminalAgentHooks: false,
  appendSystemPrompt: "",
  agentProfiles: [],
};

describe("applyDaemonConfigPatch", () => {
  it("deep-merges a provider patch and keeps the fields the patch omits", () => {
    const next = applyDaemonConfigPatch(base, { providers: { codex: { enabled: false } } });

    expect(next.providers.codex).toEqual({
      enabled: false,
      label: "Codex",
      env: { AIT: "1" },
      params: { model: "gpt" },
    });
  });

  it("merges nested provider maps instead of replacing them", () => {
    const next = applyDaemonConfigPatch(base, {
      providers: { codex: { env: { EXTRA: "2" } } },
    });

    expect(next.providers.codex?.env).toEqual({ AIT: "1", EXTRA: "2" });
  });

  it("adds a provider that the snapshot does not have yet", () => {
    const next = applyDaemonConfigPatch(base, {
      providers: { claude: { enabled: true, label: "Claude" } },
    });

    expect(next.providers.claude).toEqual({ enabled: true, label: "Claude" });
    expect(next.providers.codex).toBe(base.providers.codex);
  });

  it("deep-merges browser tools over the existing object", () => {
    const next = applyDaemonConfigPatch(base, { browserTools: { enabled: true } });

    expect(next.browserTools).toEqual({ enabled: true });
  });

  it("replaces fields the daemon replaces wholesale", () => {
    const profiles = [{ id: "profile-1", label: "One" }] as unknown as NonNullable<
      MutableDaemonConfig["agentProfiles"]
    >;
    const next = applyDaemonConfigPatch(base, {
      agentProfiles: profiles,
      appendSystemPrompt: "be terse",
    });

    expect(next.agentProfiles).toBe(profiles);
    expect(next.appendSystemPrompt).toBe("be terse");
  });

  it("applies false without treating it as an absent field", () => {
    const config = { ...base, enableTerminalAgentHooks: true };
    const next = applyDaemonConfigPatch(config, { enableTerminalAgentHooks: false });

    expect(next.enableTerminalAgentHooks).toBe(false);
  });

  it("removes a provider from the config and from metadata generation targets", () => {
    const config: MutableDaemonConfig = {
      ...base,
      providers: { ...base.providers, claude: { enabled: true } },
      metadataGeneration: {
        providers: [
          { provider: "codex", enabled: true },
          { provider: "claude", enabled: true },
        ],
      },
    };
    const next = applyDaemonConfigPatch(config, { removeProviders: ["claude"] });

    expect(next.providers).not.toHaveProperty("claude");
    expect(next.providers.codex).toEqual(config.providers.codex);
    expect(next.metadataGeneration.providers).toEqual([{ provider: "codex", enabled: true }]);
  });

  it("leaves the snapshot untouched for a field the daemon does not persist", () => {
    const patch = { skills: { enabled: true } } as unknown as Parameters<
      typeof applyDaemonConfigPatch
    >[1];
    const next = applyDaemonConfigPatch(base, patch);

    expect(next).not.toHaveProperty("skills");
    expect(next).toEqual(base);
  });
});
