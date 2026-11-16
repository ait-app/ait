import { afterEach, describe, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import {
  formatAgentPanelSubtitle,
  formatProviderSubagentSubtitle,
} from "@/panels/agent-panel-subtitle";
import { buildDraftPanelDescriptor } from "@/panels/draft-panel-descriptor";

describe("formatAgentPanelSubtitle", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("names the provider", () => {
    expect(formatAgentPanelSubtitle("claude-code")).toBe("Claude Code agent");
  });

  it("reads as the plain fallback when the provider is empty", () => {
    expect(formatAgentPanelSubtitle("")).toBe("Agent");
  });

  it("uses the active language for the fallback", async () => {
    await i18n.changeLanguage("es");
    expect(formatAgentPanelSubtitle("")).toBe(i18n.t("workspace.tabs.fallback.agent"));
  });
});

describe("formatProviderSubagentSubtitle", () => {
  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("names the provider", () => {
    expect(formatProviderSubagentSubtitle("claude-code")).toBe("Claude Code subagent");
  });

  it("reads as the plain fallback when the provider is empty", () => {
    expect(formatProviderSubagentSubtitle("")).toBe("Subagent");
  });

  it("uses the active language for the fallback", async () => {
    await i18n.changeLanguage("es");
    expect(formatProviderSubagentSubtitle("")).toBe(i18n.t("panels.providerSubagent.label"));
  });
});

function TestIcon() {
  return null;
}

describe("buildDraftPanelDescriptor", () => {
  it("uses the initial prompt title and running loader bucket during create", () => {
    const descriptor = buildDraftPanelDescriptor({
      isCreating: true,
      pendingPrompt: "Build the dashboard",
      icon: TestIcon,
    });

    expect(descriptor).toMatchObject({
      label: "Build the dashboard",
      subtitle: "Creating agent",
      titleState: "ready",
      statusBucket: "running",
    });
  });

  it("falls back to the draft title for empty create prompts", () => {
    const descriptor = buildDraftPanelDescriptor({
      isCreating: true,
      pendingPrompt: "   ",
      icon: TestIcon,
    });

    expect(descriptor.label).toBe("New Agent");
  });

  it("keeps ordinary draft tabs labeled as new agents", () => {
    const descriptor = buildDraftPanelDescriptor({ isCreating: false, icon: TestIcon });

    expect(descriptor).toMatchObject({
      label: "New Agent",
      subtitle: "New Agent",
      titleState: "ready",
      statusBucket: null,
    });
  });

  it("uses the active language for draft descriptor chrome", async () => {
    await i18n.changeLanguage("zh-CN");
    const idleDescriptor = buildDraftPanelDescriptor({
      isCreating: false,
      icon: TestIcon,
    });
    const creatingDescriptor = buildDraftPanelDescriptor({
      isCreating: true,
      pendingPrompt: "   ",
      icon: TestIcon,
    });

    expect(idleDescriptor).toMatchObject({
      label: "新建 Agent",
      subtitle: "新建 Agent",
    });
    expect(creatingDescriptor).toMatchObject({
      label: "新建 Agent",
      subtitle: "正在创建 Agent",
    });
    await i18n.changeLanguage("en");
  });
});
