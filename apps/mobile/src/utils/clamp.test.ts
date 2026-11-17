import { describe, expect, it } from "vitest";
import { clamp } from "./clamp";

describe("clamp", () => {
  it("returns the value when it is inside the range", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(0, 0, 10)).toBe(0);
    expect(clamp(10, 0, 10)).toBe(10);
  });

  it("limits the value to the nearest bound", () => {
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
  });

  it("returns max when the range is inverted", () => {
    expect(clamp(5, 10, 0)).toBe(0);
    expect(clamp(-5, 10, 0)).toBe(0);
    expect(clamp(20, 10, 0)).toBe(0);
  });

  it("propagates NaN", () => {
    expect(clamp(Number.NaN, 0, 10)).toBeNaN();
  });
});
