import { describe, expect, it } from "vitest";
import {
  CADENCE_PRESET_OPTIONS,
  normalizeScheduleFormCadence,
  resolveCronPresetId,
} from "./schedule-cadence-options";

describe("schedule cadence form options", () => {
  it("offers the approved cron preset vocabulary", () => {
    expect(CADENCE_PRESET_OPTIONS.map((option) => option.label)).toEqual([
      "Every minute",
      "Every hour",
      "Daily 9:00",
      "Weekdays 9:00",
      "Mondays 9:00",
    ]);
  });

  it("preserves fixed intervals instead of approximating them with cron", () => {
    for (const everyMs of [60_000, 5 * 60_000, 60 * 60_000, 24 * 60 * 60_000, 90_000]) {
      expect(normalizeScheduleFormCadence({ type: "every", everyMs }, "Europe/Madrid")).toEqual({
        type: "every",
        everyMs,
      });
    }
  });
  it("maps legacy single triggers to the shared interval editor and supplies cron timezones", () => {
    const once = { type: "once" as const, at: "2030-01-01T00:00:00Z" };
    expect(normalizeScheduleFormCadence(once, "Europe/Madrid")).toEqual({
      type: "every",
      everyMs: 3_600_000,
    });
    expect(
      normalizeScheduleFormCadence({ type: "cron", expression: "0 9 * * *" }, "Europe/Madrid"),
    ).toEqual({ type: "cron", expression: "0 9 * * *", timezone: "Europe/Madrid" });
    expect(resolveCronPresetId({ type: "cron", expression: "*/5 * * * *" })).toBe("custom");
  });
});
