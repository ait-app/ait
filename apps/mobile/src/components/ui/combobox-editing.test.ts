// @vitest-environment jsdom
import { expect, it } from "vitest";
import { shouldHandleComboboxKey } from "./combobox-keyboard";

function dispatchedKey(target: HTMLElement, key: string, isComposing = false) {
  const event = new KeyboardEvent("keydown", { key, isComposing, bubbles: true });
  target.dispatchEvent(event);
  return event;
}

it("does not take arrows or Enter from native form editors, including contenteditable", () => {
  for (const element of [
    document.createElement("textarea"),
    document.createElement("input"),
    document.createElement("div"),
  ]) {
    if (element.tagName === "DIV") element.contentEditable = "true";
    if (element.tagName === "DIV") element.setAttribute("contenteditable", "true");
    for (const key of ["ArrowDown", "ArrowUp", "Enter"])
      expect(shouldHandleComboboxKey(dispatchedKey(element, key))).toBe(false);
    expect(shouldHandleComboboxKey(dispatchedKey(element, "Escape"))).toBe(true);
  }
});

it("retains search navigation and yields Escape during IME composition", () => {
  const search = document.createElement("div");
  search.dataset.comboboxSearch = "";
  const input = document.createElement("input");
  search.append(input);
  expect(shouldHandleComboboxKey(dispatchedKey(input, "ArrowDown"))).toBe(true);
  expect(shouldHandleComboboxKey(dispatchedKey(input, "Escape", true))).toBe(false);
  expect(shouldHandleComboboxKey(dispatchedKey(document.createElement("button"), "ArrowUp"))).toBe(
    true,
  );
});
