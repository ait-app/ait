import { describe, expect, it } from "vitest";

import { filterAndRankComboboxOptions } from "./combobox-options";

describe("filterAndRankComboboxOptions", () => {
  const options = [
    { id: "feat/login", label: "feat/login" },
    { id: "main", label: "main" },
    { id: "feat/main-nav", label: "feat/main-nav" },
    { id: "fix/logout", label: "fix/logout", description: "fixes main logout bug" },
  ];

  it("returns all options when search is empty", () => {
    expect(filterAndRankComboboxOptions(options, "")).toEqual(options);
  });

  it("filters by label substring", () => {
    const result = filterAndRankComboboxOptions(options, "login");
    expect(result.map((o) => o.id)).toEqual(["feat/login"]);
  });

  it("filters by id substring", () => {
    const result = filterAndRankComboboxOptions(options, "fix/");
    expect(result.map((o) => o.id)).toEqual(["fix/logout"]);
  });

  it("filters by description substring", () => {
    const result = filterAndRankComboboxOptions(options, "logout bug");
    expect(result.map((o) => o.id)).toEqual(["fix/logout"]);
  });

  it("ranks prefix matches above substring matches", () => {
    const result = filterAndRankComboboxOptions(options, "main");
    expect(result.map((o) => o.id)).toEqual(["main", "feat/main-nav", "fix/logout"]);
  });

  it("is case-insensitive", () => {
    const items = [{ id: "Alpha", label: "Alpha" }];
    expect(filterAndRankComboboxOptions(items, "alpha")).toHaveLength(1);
  });

  it("returns empty when nothing matches", () => {
    expect(filterAndRankComboboxOptions(options, "zzz")).toEqual([]);
  });

  it("ranks word-boundary matches above mid-word substring matches", () => {
    const items = [
      { id: "happy", label: "happy" },
      { id: "a/py", label: "a/py" },
    ];
    const result = filterAndRankComboboxOptions(items, "py");
    expect(result.map((o) => o.id)).toEqual(["a/py", "happy"]);
  });

  it("interleaves mixed branches and PRs by match quality", () => {
    const items = [
      { id: "branch:feat/api-login", label: "feat/api-login" },
      { id: "branch:feat/pi-direct-sdk", label: "feat/pi-direct-sdk" },
      { id: "github-pr:202", label: "#202 feat(server): replace Pi ACP with direct SDK provider" },
      { id: "github-pr:355", label: "#355 feat: add LaTeX math formula rendering" },
    ];
    const result = filterAndRankComboboxOptions(items, "pi");
    expect(result.map((o) => o.id)).toEqual([
      "branch:feat/pi-direct-sdk",
      "github-pr:202",
      "branch:feat/api-login",
    ]);
  });

  it("ranks PR-number matches via word-boundary on #", () => {
    const items = [
      { id: "github-pr:202", label: "#202 some title" },
      { id: "github-pr:1202", label: "#1202 another title" },
    ];
    const result = filterAndRankComboboxOptions(items, "202");
    expect(result.map((o) => o.id)).toEqual(["github-pr:202", "github-pr:1202"]);
  });

  it("matches fuzzy character sequences after stronger substring matches", () => {
    const items = [
      { id: "gpt-5.4", label: "GPT-5.4" },
      { id: "gpt-4.1", label: "GPT-4.1" },
      { id: "gemini", label: "Gemini" },
    ];
    const result = filterAndRankComboboxOptions(items, "gpt54");
    expect(result.map((o) => o.id)).toEqual(["gpt-5.4"]);
  });
});
