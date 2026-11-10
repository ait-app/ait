import { describe, expect, it } from "vitest";
import { i18n } from "@/i18n/i18next";
import { validateCron } from "@/utils/schedule-format";
import {
  buildScheduleTemplateDraft,
  getScheduleTemplateCadenceLabel,
  findScheduleTemplate,
  isScheduleTemplateReady,
  SCHEDULE_TEMPLATES,
} from "./schedule-templates";

describe("schedule templates", () => {
  it("defines valid cron cadences for every template", () => {
    for (const template of SCHEDULE_TEMPLATES) {
      expect(validateCron(template.cronExpression)).toBeNull();
    }
  });

  it("builds an editable recurring draft in the requested timezone", () => {
    const template = findScheduleTemplate("pr-gatekeeper")!;
    const draft = buildScheduleTemplateDraft({ template, params: {}, timezone: "Asia/Shanghai" });
    expect(draft).toEqual({
      name: "PR risk review",
      prompt: expect.stringContaining("open PRs"),
      cadence: { type: "cron", expression: "0 10 * * 1-5", timezone: "Asia/Shanghai" },
      maxRuns: "",
    });
  });

  it("requires and inserts trimmed template parameters", () => {
    const template = findScheduleTemplate("domain-radar")!;
    expect(isScheduleTemplateReady(template, {})).toBe(false);
    expect(isScheduleTemplateReady(template, { topic: "  " })).toBe(false);
    expect(isScheduleTemplateReady(template, { topic: "Rust async" })).toBe(true);
    const draft = buildScheduleTemplateDraft({
      template,
      params: { topic: "  Rust async " },
      timezone: "UTC",
    });
    expect(draft.prompt).toContain("“Rust async”");
    expect(draft.prompt).not.toContain("{{");
  });

  it("builds a localized prompt and preserves literal parameter text", () => {
    const draft = buildScheduleTemplateDraft({
      template: findScheduleTemplate("domain-radar")!,
      params: { topic: "  Rust <async> & $&  " },
      timezone: "Asia/Shanghai",
      t: i18n.getFixedT("zh-CN"),
    });
    expect(draft.name).toBe("主题简报");
    expect(draft.prompt).toContain("「Rust <async> & $&」");
    expect(draft.prompt).not.toContain("{{topic}}");
  });

  it("labels daily and monthly schedules in the active language", () => {
    expect(getScheduleTemplateCadenceLabel(findScheduleTemplate("nightly-test-heal")!)).toBe(
      "Daily · 02:00",
    );
    expect(
      getScheduleTemplateCadenceLabel(
        findScheduleTemplate("docs-freshness")!,
        i18n.getFixedT("zh-CN"),
      ),
    ).toBe("每月 1 日 · 10:00");
    expect(findScheduleTemplate("missing")).toBeNull();
  });
});
