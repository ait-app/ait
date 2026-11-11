// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { shouldHandleComboboxKey } from "./combobox-keyboard";

function keyEvent(overrides: Partial<KeyboardEvent> & { target?: unknown }): KeyboardEvent {
  return {
    key: "ArrowDown",
    isComposing: false,
    keyCode: 40,
    target: null,
    ...overrides,
  } as KeyboardEvent;
}

const attached: Element[] = [];

function attach(element: Element): Element {
  document.body.append(element);
  attached.push(element);
  return element;
}

afterEach(() => {
  for (const element of attached.splice(0)) element.remove();
});

describe("shouldHandleComboboxKey", () => {
  it("never navigates during IME composition", () => {
    expect(shouldHandleComboboxKey(keyEvent({ isComposing: true }))).toBe(false);
    expect(shouldHandleComboboxKey(keyEvent({ keyCode: 229 }))).toBe(false);
  });

  it("never commits or dismisses the menu mid-composition, even from its own search", () => {
    const search = attach(document.createElement("div"));
    search.setAttribute("data-combobox-search", "");
    const searchInput = attach(document.createElement("input"));
    search.append(searchInput);

    for (const key of ["Enter", "Escape", "ArrowDown", "ArrowUp"]) {
      expect(
        shouldHandleComboboxKey(keyEvent({ key, isComposing: true, target: searchInput })),
      ).toBe(false);
      expect(shouldHandleComboboxKey(keyEvent({ key, keyCode: 229, target: searchInput }))).toBe(
        false,
      );
    }
  });

  it("keeps Escape available everywhere", () => {
    const editor = attach(document.createElement("textarea"));
    expect(shouldHandleComboboxKey(keyEvent({ key: "Escape", target: editor }))).toBe(true);
  });

  it("yields arrow navigation to text editors but keeps its own search", () => {
    const editor = attach(document.createElement("textarea"));
    const search = attach(document.createElement("div"));
    search.setAttribute("data-combobox-search", "");
    const searchInput = attach(document.createElement("input"));
    search.append(searchInput);

    expect(shouldHandleComboboxKey(keyEvent({ target: editor }))).toBe(false);
    expect(shouldHandleComboboxKey(keyEvent({ target: searchInput }))).toBe(true);
    expect(shouldHandleComboboxKey(keyEvent({ target: document.body }))).toBe(true);
  });

  it("treats contenteditable and textbox roles as text editing", () => {
    const editable = attach(document.createElement("div"));
    editable.setAttribute("contenteditable", "true");
    const textbox = attach(document.createElement("div"));
    textbox.setAttribute("role", "textbox");

    expect(shouldHandleComboboxKey(keyEvent({ target: editable }))).toBe(false);
    expect(shouldHandleComboboxKey(keyEvent({ target: textbox }))).toBe(false);
  });

  it("handles keys when there is no DOM target yet", () => {
    expect(shouldHandleComboboxKey(keyEvent({ target: null }))).toBe(true);
  });
});
