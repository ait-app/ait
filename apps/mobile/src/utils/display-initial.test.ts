import { describe, expect, it } from "vitest";
import { displayInitial } from "./display-initial";

describe("displayInitial", () => {
  it("keeps an astral first character whole", () => {
    expect(displayInitial("\u{1F600}App")).toBe("\u{1F600}");
  });

  it("uppercases a letter", () => {
    expect(displayInitial("paseo")).toBe("P");
  });

  it("returns an empty string for an empty value", () => {
    expect(displayInitial("")).toBe("");
  });
});
