import { describe, expect, it } from "vitest";

import { compactHighlightTokens } from "./diff-rendering";

describe("diff-rendering", () => {
  it("compacts adjacent syntax tokens without changing their text or styles", () => {
    expect(
      compactHighlightTokens([
        { text: "const", style: "keyword" },
        { text: " ", style: null },
        { text: "value", style: null },
        { text: " ", style: null },
        { text: "=", style: "operator" },
        { text: "", style: "operator" },
        { text: " 1", style: "number" },
      ]),
    ).toEqual([
      { text: "const", style: "keyword" },
      { text: " value ", style: null },
      { text: "=", style: "operator" },
      { text: " 1", style: "number" },
    ]);
  });
});
