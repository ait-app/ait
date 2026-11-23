import { describe, expect, it } from "vitest";

import { splitUserMessageTags, userMessageProse } from "./user-message-tags";

describe("splitUserMessageTags", () => {
  it("separates an injected reminder from the prompt that follows it", () => {
    const message = [
      "<system-reminder>",
      "You are operating in a git worktree.",
      "Worktree path: /tmp/wt",
      "</system-reminder>",
      "",
      "Fix the failing test.",
    ].join("\n");

    expect(splitUserMessageTags(message)).toEqual([
      {
        kind: "tag",
        name: "system-reminder",
        body: "You are operating in a git worktree.\nWorktree path: /tmp/wt",
      },
      { kind: "text", text: "Fix the failing test." },
    ]);
  });

  it("keeps prose around several blocks in order and reads attributed tags", () => {
    const message = [
      "Look at this:",
      '<pasted_content id="a1">',
      "line one",
      "</pasted_content>",
      "and this",
      '<system-reminder reason="todo">Stopped early</system-reminder>',
    ].join("\n");

    expect(splitUserMessageTags(message)).toEqual([
      { kind: "text", text: "Look at this:" },
      { kind: "tag", name: "pasted_content", body: "line one" },
      { kind: "text", text: "and this" },
      { kind: "tag", name: "system-reminder", body: "Stopped early" },
    ]);
  });

  it("balances nested tags of the same name", () => {
    const message = "<outer-block>\n<outer-block>inner</outer-block>\n</outer-block>";

    expect(splitUserMessageTags(message)).toEqual([
      { kind: "tag", name: "outer-block", body: "<outer-block>inner</outer-block>" },
    ]);
  });

  it.each([
    ["an inline mention", "what does <system-reminder>x</system-reminder> do?"],
    ["a tag closed mid-line", "<system-reminder>x</system-reminder> trailing"],
    ["an unclosed tag", "<system-reminder>\nnever closed"],
    ["an HTML element", "<div>\nhello\n</div>"],
    ["plain text", "just a prompt"],
  ])("leaves %s as text", (_case, message) => {
    expect(splitUserMessageTags(message)).toEqual([{ kind: "text", text: message }]);
  });
});

describe("userMessageProse", () => {
  it("drops tag blocks and keeps the prompt", () => {
    expect(userMessageProse("<system-reminder>\nnoise\n</system-reminder>\nFix it")).toBe("Fix it");
  });

  it("is empty for a message that holds only blocks", () => {
    expect(userMessageProse("<system-reminder>noise</system-reminder>")).toBe("");
  });
});
