import { afterEach, describe, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import { formatAgentFeatureLabel, formatAgentModeLabel, formatThinkingOptionLabel } from "./labels";

afterEach(async () => {
  await i18n.changeLanguage("en");
});

describe("formatAgentModeLabel", () => {
  it("sentence-cases provider mode labels", () => {
    expect(formatAgentModeLabel({ id: "plan", label: "Plan" })).toBe("Plan");
    expect(formatAgentModeLabel({ id: "full-access", label: "Full Access" })).toBe("Full access");
    expect(formatAgentModeLabel({ id: "auto-review", label: "Auto-review" })).toBe("Auto-review");
    expect(formatAgentModeLabel({ id: "read_only", label: "read_only" })).toBe("Read only");
    expect(formatAgentModeLabel({ id: "acceptEdits", label: "acceptEdits" })).toBe("Accept edits");
  });

  it("splits compact mode ids when no provider label is available", () => {
    expect(formatAgentModeLabel({ id: "auto-review" })).toBe("Auto review");
  });
});

describe("localized OMP controls", () => {
  it("translates Chinese mode, effort and feature labels while keeping protocol ids intact", async () => {
    await i18n.changeLanguage("zh-CN");
    expect(formatAgentModeLabel({ id: "full", label: "Full Access" })).toBe("完全访问");
    expect(formatAgentModeLabel({ id: "write", label: "Allow File Writes" })).toBe("允许写入文件");
    expect(formatAgentModeLabel({ id: "ask", label: "Always Ask" })).toBe("始终询问");
    expect(formatAgentModeLabel({ id: "ask-cached", label: "Ask and Remember" })).toBe(
      "询问并记住",
    );
    expect(formatThinkingOptionLabel({ id: "off", label: "off" })).toBe("关闭");
    expect(formatThinkingOptionLabel({ id: "max", label: "max" })).toBe("最高");
    expect(formatThinkingOptionLabel({ id: "xhigh", label: "xhigh" })).toBe("极高");
    for (const [id, label] of [
      ["autoCompaction", "自动压缩上下文"],
      ["autoRetry", "自动重试"],
      ["fastMode", "快速服务档位"],
      ["slowMode", "慢速服务档位"],
    ]) {
      expect(formatAgentFeatureLabel({ id, label: "Native English label" })).toBe(label);
    }
  });

  it.each(["ar", "es", "fr", "ja", "ko", "pt-BR", "ru", "zh-CN"])(
    "localizes standard OMP labels in %s",
    async (language) => {
      await i18n.changeLanguage(language);
      expect(formatThinkingOptionLabel({ id: "max" })).toBe(i18n.t("schedules.thinking.max"));
      expect(formatAgentFeatureLabel({ id: "autoRetry", label: "Automatic retry" })).not.toBe(
        "Automatic retry",
      );
      expect(formatAgentModeLabel({ id: "write", label: "Allow File Writes" })).not.toBe(
        "Allow file writes",
      );
    },
  );

  it("retains custom provider labels and follows language changes", async () => {
    const feature = { id: "autoRetry", label: "Automatic retry" };
    expect(formatAgentFeatureLabel(feature)).toBe("Automatic retry");
    await i18n.changeLanguage("zh-CN");
    expect(formatAgentFeatureLabel(feature)).toBe("自动重试");
    expect(feature.label).toBe("Automatic retry");
    expect(formatAgentFeatureLabel({ id: "custom", label: "My custom control" })).toBe(
      "My custom control",
    );
    expect(formatAgentModeLabel({ id: "full", label: "Custom mode" })).toBe("Custom mode");
    expect(formatThinkingOptionLabel({ id: "max", label: "Custom effort" })).toBe("Custom effort");
  });
});

describe("formatThinkingOptionLabel", () => {
  it("formats compact thinking option labels for display", () => {
    expect(formatThinkingOptionLabel({ id: "none", label: "none" })).toBe("None");
    expect(formatThinkingOptionLabel({ id: "low", label: "low" })).toBe("Low");
    expect(formatThinkingOptionLabel({ id: "medium", label: "medium" })).toBe("Medium");
    expect(formatThinkingOptionLabel({ id: "high", label: "high" })).toBe("High");
    expect(formatThinkingOptionLabel({ id: "xhigh", label: "xhigh" })).toBe("Extra high");
  });

  it("sentence-cases split provider labels", () => {
    expect(formatThinkingOptionLabel({ id: "extra_high", label: "extra_high" })).toBe("Extra high");
    expect(formatThinkingOptionLabel({ id: "think-hard", label: "think-hard" })).toBe("Think hard");
    expect(formatThinkingOptionLabel({ id: "xhigh", label: "XHigh" })).toBe("Extra high");
  });
});
