import { describe, expect, it } from "vitest";
import { isRecord } from "./is-record";

describe("isRecord", () => {
  it("accepts plain objects", () => {
    expect(isRecord({})).toBe(true);
    expect(isRecord({ key: "value" })).toBe(true);
    expect(isRecord(Object.create(null))).toBe(true);
  });

  it("rejects arrays", () => {
    expect(isRecord([])).toBe(false);
    expect(isRecord([{ key: "value" }])).toBe(false);
  });

  it("rejects null and undefined", () => {
    expect(isRecord(null)).toBe(false);
    expect(isRecord(undefined)).toBe(false);
  });

  it("rejects primitives and functions", () => {
    expect(isRecord("value")).toBe(false);
    expect(isRecord(1)).toBe(false);
    expect(isRecord(true)).toBe(false);
    expect(isRecord(() => undefined)).toBe(false);
  });
});
