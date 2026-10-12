import { describe, expect, it } from "vitest";
import {
  formatLocalDateTime,
  isValidMaxRuns,
  isValidScheduleTimezone,
  parseLocalDateTime,
} from "./schedule-date-time";

describe("schedule calendar input", () => {
  it("round-trips local calendar time independently of the device UTC offset", () => {
    const local = "2030-02-28 09:35";
    const iso = parseLocalDateTime(local);
    expect(iso).not.toBeNull();
    expect(formatLocalDateTime(iso)).toBe(local);
    expect(parseLocalDateTime("2032-02-29T12:00")).not.toBeNull();
  });
  it.each([
    "2030-02-29 12:00",
    "2030-13-01 09:00",
    "2030-01-32 09:00",
    "2030-01-01 24:00",
    "2030-01-01 09:60",
    "tomorrow",
    "2030-01-01",
  ])("rejects invalid calendar input %s", (input) => {
    expect(parseLocalDateTime(input)).toBeNull();
  });
  it("handles empty or invalid stored dates", () => {
    expect(formatLocalDateTime(null)).toBe("");
    expect(formatLocalDateTime("invalid")).toBe("");
  });
  it("accepts supported IANA zones and rejects invalid zones", () => {
    expect(isValidScheduleTimezone("Asia/Shanghai")).toBe(true);
    expect(isValidScheduleTimezone("UTC")).toBe(true);
    expect(isValidScheduleTimezone("Mars/Office")).toBe(false);
    expect(isValidScheduleTimezone("")).toBe(false);
  });
});

describe("schedule execution limit", () => {
  it.each(["", "1", "20", " 2 "])("accepts whole numbers or unlimited: %s", (value) => {
    expect(isValidMaxRuns(value)).toBe(true);
  });
  it.each(["0", "-1", "1.5", "1e3", "2abc", "Infinity", "9007199254740992"])(
    "rejects invalid limits: %s",
    (value) => {
      expect(isValidMaxRuns(value)).toBe(false);
    },
  );
});
