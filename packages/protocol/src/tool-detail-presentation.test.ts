import { describe, expect, it } from "vitest";

import { buildToolDetailGroups, summarizeUnknownToolInput } from "./tool-detail-presentation.js";

describe("unknown tool detail layout", () => {
  it("lays out a skill call as a field and a line of text", () => {
    expect(
      buildToolDetailGroups(
        "Skill",
        { skill: "superpowers:brainstorming" },
        "Launching skill: superpowers:brainstorming",
      ),
    ).toEqual([
      {
        role: "input",
        blocks: [
          { kind: "fields", fields: [{ label: "Skill", value: "superpowers:brainstorming" }] },
        ],
      },
      {
        role: "output",
        blocks: [{ kind: "text", text: "Launching skill: superpowers:brainstorming" }],
      },
    ]);
  });

  it("shows REPL code as highlighted code and unwraps the MCP result", () => {
    const code = "await tab.goto('https://example.com')\ntext(await tab.title())";
    expect(
      buildToolDetailGroups(
        "cua_repl.js",
        { code },
        {
          content: [
            { type: "text", text: "Script completed\nWall time 0.6 seconds\nOutput:\n" },
            { type: "text", text: "[Image]" },
            { type: "text", text: "[Image]" },
          ],
        },
      ),
    ).toEqual([
      { role: "input", blocks: [{ kind: "code", label: "Code", text: code, language: "js" }] },
      {
        role: "output",
        blocks: [
          { kind: "code", text: "Script completed\nWall time 0.6 seconds\nOutput:" },
          { kind: "images", count: 2 },
        ],
      },
    ]);
  });

  it("keeps a one-line code argument in a code block", () => {
    expect(buildToolDetailGroups("cua_repl.js", { code: "tab.reload()" }, null)).toEqual([
      {
        role: "input",
        blocks: [{ kind: "code", label: "Code", text: "tab.reload()", language: "js" }],
      },
    ]);
  });

  it("reads JSON text and structured content as fields", () => {
    expect(
      buildToolDetailGroups("mcp__github__get_issue", { number: 7 }, [
        { type: "text", text: '{"title":"Crash on start","state":"open"}' },
      ]),
    ).toEqual([
      { role: "input", blocks: [{ kind: "fields", fields: [{ label: "Number", value: "7" }] }] },
      {
        role: "output",
        blocks: [
          {
            kind: "fields",
            fields: [
              { label: "Title", value: "Crash on start" },
              { label: "State", value: "open" },
            ],
          },
        ],
      },
    ]);
    expect(
      buildToolDetailGroups(undefined, null, {
        content: [{ type: "text", text: "ignored" }],
        structuredContent: { ok: true },
      }),
    ).toEqual([
      { role: "output", blocks: [{ kind: "fields", fields: [{ label: "Ok", value: "Yes" }] }] },
    ]);
  });

  it("gives long prose its own block and falls back to JSON for deep values", () => {
    const prompt = "Review the change.\nFocus on error handling.";
    const deep = { a: { b: { c: { d: 1 } } } };
    expect(buildToolDetailGroups("mcp__x__run", { prompt, options: deep }, null)).toEqual([
      {
        role: "input",
        blocks: [
          { kind: "text", label: "Prompt", text: prompt },
          {
            kind: "code",
            label: "Options",
            text: JSON.stringify(deep, null, 2),
            language: "json",
          },
        ],
      },
    ]);
  });

  it("never shows image data and keeps unknown content blocks as JSON", () => {
    const groups = buildToolDetailGroups("mcp__x__shot", {}, [
      { type: "image", data: "aGVsbG8=", mimeType: "image/png" },
      { type: "resource", resource: { uri: "file:///a.txt" } },
    ]);
    expect(groups).toEqual([
      {
        role: "output",
        blocks: [
          { kind: "images", count: 1 },
          {
            kind: "code",
            text: JSON.stringify({ type: "resource", resource: { uri: "file:///a.txt" } }, null, 2),
            language: "json",
          },
        ],
      },
    ]);
    expect(JSON.stringify(groups)).not.toContain("aGVsbG8=");
  });

  it("does not mistake a plain object with a content field for an MCP envelope", () => {
    expect(
      buildToolDetailGroups("mcp__x__note", {}, { content: "Saved.", revision: "r2" }),
    ).toEqual([
      {
        role: "output",
        blocks: [
          {
            kind: "fields",
            fields: [
              { label: "Content", value: "Saved." },
              { label: "Revision", value: "r2" },
            ],
          },
        ],
      },
    ]);
  });
});

describe("unknown tool summary", () => {
  it.each([
    [{ skill: "superpowers:brainstorming" }, "superpowers:brainstorming"],
    [
      { code: "\nawait tab.goto('https://example.com')\ntext(1)" },
      "await tab.goto('https://example.com')",
    ],
    [{ limit: 3, owner: "ait" }, "ait"],
    ["first line\nsecond", "first line"],
  ])("names %j by %s", (input, summary) => {
    expect(summarizeUnknownToolInput(input)).toBe(summary);
  });

  it("has no summary without a string argument", () => {
    expect(summarizeUnknownToolInput({ limit: 3 })).toBeUndefined();
    expect(summarizeUnknownToolInput(null)).toBeUndefined();
  });
});
