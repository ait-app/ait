import { describe, expect, it } from "vitest";

import {
  buildLineDiff,
  parseUnifiedDiff,
  extractTaskEntriesFromToolCall,
} from "./tool-call-parsers";

describe("tool-call-parsers", () => {
  it("builds line diff for text changes", () => {
    const diff = buildLineDiff("old\nline\n", "new\nline\n");

    expect(diff.some((entry) => entry.type === "remove")).toBe(true);
    expect(diff.some((entry) => entry.type === "add")).toBe(true);
  });

  it("parses unified diff", () => {
    const parsed = parseUnifiedDiff("@@\n-old\n+new\n");

    expect(parsed.find((entry) => entry.type === "remove")?.content).toBe("-old");
    expect(parsed.find((entry) => entry.type === "add")?.content).toBe("+new");
  });

  it("keeps hunk body lines that start with +++ or ---", () => {
    const parsed = parseUnifiedDiff("@@ -1 +1 @@\n--- a\n+++b");

    expect(parsed.filter((entry) => entry.type === "remove").map((entry) => entry.content)).toEqual(
      ["--- a"],
    );
    expect(parsed.filter((entry) => entry.type === "add").map((entry) => entry.content)).toEqual([
      "+++b",
    ]);
  });

  it("treats --- and +++ as headers before each file section's first hunk", () => {
    const parsed = parseUnifiedDiff(
      [
        "--- a/x.ts",
        "+++ b/x.ts",
        "@@ -1 +1 @@",
        "-one",
        "+two",
        "--- a/y.ts",
        "+++ b/y.ts",
        "@@ -1 +1 @@",
        "-three",
        "+four",
        "diff --git a/z.ts b/z.ts",
        "--- a/z.ts",
        "+++ b/z.ts",
        "@@",
        "-five",
      ].join("\n"),
    );

    expect(parsed.filter((entry) => entry.type === "remove").map((entry) => entry.content)).toEqual(
      ["-one", "-three", "-five"],
    );
    expect(parsed.filter((entry) => entry.type === "add").map((entry) => entry.content)).toEqual([
      "+two",
      "+four",
    ]);
  });

  it("replaces the whole text without an LCS table above the line budget", () => {
    const original = Array.from({ length: 2_100 }, (_, index) => `line-${index}`).join("\n");
    const diff = buildLineDiff(original, `${original}\nextra`);

    expect(diff.map((entry) => entry.type)).toEqual([
      ...Array<string>(2_100).fill("remove"),
      ...Array<string>(2_101).fill("add"),
    ]);
    expect(diff.every((entry) => entry.segments === undefined)).toBe(true);
  });

  it("skips word segments for a line pair above the word budget", () => {
    const words = Array.from({ length: 2_500 }, (_, index) => `word${index}`);
    const diff = buildLineDiff(words.join(" "), [...words.slice(1), "tail"].join(" "));

    expect(diff.map((entry) => entry.type)).toEqual(["remove", "add"]);
    expect(diff.every((entry) => entry.segments === undefined)).toBe(true);
  });

  it("stops adding word segments once the shared word budget is spent", () => {
    const pairCount = 900;
    const words = (label: string) =>
      Array.from({ length: 50 }, (_, index) => `${label}${index}`).join(" ");
    const lines = (label: string) =>
      Array.from({ length: pairCount }, (_, index) => [`context-${index}`, words(label)]).flat();

    const diff = buildLineDiff(lines("old").join("\n"), lines("new").join("\n"));
    const removes = diff.filter((entry) => entry.type === "remove");

    expect(removes).toHaveLength(pairCount);
    expect(removes[0]?.segments).toBeDefined();
    expect(removes[pairCount - 1]?.segments).toBeUndefined();
  });

  it("extracts TodoWrite task entries", () => {
    const tasks = extractTaskEntriesFromToolCall("TodoWrite", {
      todos: [
        { content: "Task 1", status: "pending" },
        { content: "Task 2", status: "completed" },
      ],
    });

    expect(tasks?.map((task) => task.text)).toEqual(["Task 1", "Task 2"]);
    expect(tasks?.map((task) => task.completed)).toEqual([false, true]);
  });

  it("preserves the subject and active form separately for running task tools", () => {
    expect(
      extractTaskEntriesFromToolCall("TodoWrite", {
        todos: [
          { content: " Verify support ", activeForm: " Verifying support ", status: "in_progress" },
        ],
      }),
    ).toEqual([
      {
        text: "Verify support",
        activeForm: "Verifying support",
        status: "in_progress",
        completed: false,
      },
    ]);
  });
});
