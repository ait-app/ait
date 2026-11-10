import type { TFunction } from "i18next";
import { i18n } from "@/i18n/i18next";
import type { ScheduleFormCadence } from "./schedule-cadence-options";

export type ScheduleTemplateId =
  | "nightly-test-heal"
  | "pr-gatekeeper"
  | "domain-radar"
  | "weekly-work-draft"
  | "docs-freshness";

export type ScheduleTemplateParamKey = "topic";

export type ScheduleTemplateParams = Partial<Record<ScheduleTemplateParamKey, string>>;

export const SCHEDULE_TEMPLATE_CATEGORIES = ["development", "research", "writing"] as const;
export type ScheduleTemplateCategory = (typeof SCHEDULE_TEMPLATE_CATEGORIES)[number];
export type ScheduleTemplateCapability = "code" | "pr" | "web" | "params";

export interface ScheduleTemplate {
  id: ScheduleTemplateId;
  category: ScheduleTemplateCategory;
  cronExpression: string;
  params: readonly ScheduleTemplateParamKey[];
  capabilities: readonly ScheduleTemplateCapability[];
}

export interface ScheduleTemplateDraft {
  name: string;
  prompt: string;
  cadence: ScheduleFormCadence;
  maxRuns: string;
}

export const SCHEDULE_TEMPLATES: readonly ScheduleTemplate[] = [
  {
    id: "nightly-test-heal",
    category: "development",
    cronExpression: "0 2 * * *",
    params: [],
    capabilities: ["code", "pr"],
  },
  {
    id: "pr-gatekeeper",
    category: "development",
    cronExpression: "0 10 * * 1-5",
    params: [],
    capabilities: ["pr"],
  },
  {
    id: "domain-radar",
    category: "research",
    cronExpression: "0 9 * * 1-5",
    params: ["topic"],
    capabilities: ["web", "params"],
  },
  {
    id: "weekly-work-draft",
    category: "writing",
    cronExpression: "0 16 * * 5",
    params: [],
    capabilities: ["pr"],
  },
  {
    id: "docs-freshness",
    category: "writing",
    cronExpression: "0 10 1 * *",
    params: [],
    capabilities: [],
  },
];

export function findScheduleTemplate(id: string): ScheduleTemplate | null {
  return SCHEDULE_TEMPLATES.find((template) => template.id === id) ?? null;
}

export function isScheduleTemplateReady(
  template: ScheduleTemplate,
  params: ScheduleTemplateParams,
): boolean {
  return template.params.every((key) => Boolean(params[key]?.trim()));
}

export function getScheduleTemplateCadenceLabel(
  template: ScheduleTemplate,
  t: TFunction = i18n.t,
): string {
  return t(`schedules.templates.items.${template.id}.cadence`);
}

export function buildScheduleTemplateDraft(input: {
  template: ScheduleTemplate;
  params: ScheduleTemplateParams;
  timezone: string;
  t?: TFunction;
}): ScheduleTemplateDraft {
  const { template, timezone, t = i18n.t } = input;
  const values = Object.fromEntries(
    template.params.map((key) => [key, input.params[key]?.trim() ?? ""]),
  );
  return {
    name: t(`schedules.templates.items.${template.id}.name`),
    prompt: t(`schedules.templates.items.${template.id}.prompt`, values),
    cadence: { type: "cron", expression: template.cronExpression, timezone },
    maxRuns: "",
  };
}
