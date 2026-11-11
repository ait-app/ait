import { describe, expect, it } from "vitest";
import type { CommandCompletionItem } from "@ait/protocol/messages";
import { applyNativeCommandCompletion } from "./native-command-completion";

function item(overrides: Partial<CommandCompletionItem> = {}): CommandCompletionItem {
  return {
    id: "review",
    label: "/review",
    insertText: "/review ",
    replaceRange: { start: 0, end: 5 },
    triggerCompletion: false,
    ...overrides,
  };
}

describe("applyNativeCommandCompletion", () => {
  it("applies the exact native edit and default caret", () => {
    expect(applyNativeCommandCompletion("/revi", item())).toEqual({
      text: "/review ",
      cursor: 8,
    });
  });

  it("honors an explicit native caret and preserves surrounding text", () => {
    expect(
      applyNativeCommandCompletion(
        "/revi suffix",
        item({ replaceRange: { start: 0, end: 5 }, cursor: 8 }),
      ),
    ).toEqual({ text: "/review  suffix", cursor: 8 });
  });

  it("rejects ranges that bisect surrogate pairs or exceed the text", () => {
    expect(
      applyNativeCommandCompletion("😀x", item({ replaceRange: { start: 1, end: 2 } })),
    ).toBeNull();
    expect(
      applyNativeCommandCompletion("/revi", item({ replaceRange: { start: 0, end: 99 } })),
    ).toBeNull();
    expect(
      applyNativeCommandCompletion("/revi", item({ replaceRange: { start: 4, end: 2 } })),
    ).toBeNull();
  });

  it("rejects an explicit caret outside the edited text", () => {
    expect(applyNativeCommandCompletion("/revi", item({ cursor: 99 }))).toBeNull();
  });

  it("rejects negative and fractional offsets instead of slicing by unit value", () => {
    expect(
      applyNativeCommandCompletion("/revi", item({ replaceRange: { start: -1, end: 2 } })),
    ).toBeNull();
    expect(
      applyNativeCommandCompletion("/revi", item({ replaceRange: { start: 0, end: 1.5 } })),
    ).toBeNull();
    expect(applyNativeCommandCompletion("/revi", item({ cursor: -1 }))).toBeNull();
  });

  it("lands the default caret past a multi-unit insertion without splitting it", () => {
    expect(
      applyNativeCommandCompletion(
        "/deploy ",
        item({ insertText: "pröd 😀", replaceRange: { start: 8, end: 8 } }),
      ),
    ).toEqual({ text: "/deploy pröd 😀", cursor: 15 });
  });

  it("rejects a native caret that would land inside the inserted surrogate pair", () => {
    expect(
      applyNativeCommandCompletion(
        "/revi",
        item({ insertText: "😀", replaceRange: { start: 0, end: 5 }, cursor: 1 }),
      ),
    ).toBeNull();
  });

  it("applies a pure deletion when the native edit inserts nothing", () => {
    expect(
      applyNativeCommandCompletion(
        "/review now",
        item({ insertText: "", replaceRange: { start: 7, end: 11 } }),
      ),
    ).toEqual({ text: "/review", cursor: 7 });
  });

  it("keeps quote and suffix characters the native runtime chose verbatim", () => {
    expect(
      applyNativeCommandCompletion(
        "/de",
        item({ insertText: '"/deploy prod"', replaceRange: { start: 0, end: 3 }, cursor: 13 }),
      ),
    ).toEqual({ text: '"/deploy prod"', cursor: 13 });
  });
});
