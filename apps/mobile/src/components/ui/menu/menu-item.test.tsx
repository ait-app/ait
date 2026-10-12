/**
 * @vitest-environment jsdom
 */
import React from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MenuItem } from "./menu-item";
import { MenuRoot } from "./menu-root";

beforeEach(() => vi.stubGlobal("React", React));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderMenu() {
  const view = render(
    <MenuRoot defaultOpen>
      <MenuItem>Copy path</MenuItem>
      <MenuItem>Mark as read</MenuItem>
      <MenuItem disabled>Archive</MenuItem>
    </MenuRoot>,
  );
  const [first, second, disabled] = view.getAllByRole("menuitem");
  act(() => first.focus());
  return { first, second, disabled };
}

function background(item: HTMLElement) {
  return window.getComputedStyle(item).backgroundColor;
}

describe("MenuItem highlighting", () => {
  it("moves the first item's focus and highlight to the item under the pointer", () => {
    const { first, second } = renderMenu();
    const highlight = background(first);
    expect(background(second)).not.toBe(highlight);

    fireEvent.mouseEnter(second);

    expect(document.activeElement).toBe(second);
    expect(background(second)).toBe(highlight);
    expect(background(first)).not.toBe(highlight);
  });

  it("shows only keyboard focus with a stationary pointer, then follows pointer movement", () => {
    const { first, second } = renderMenu();
    const highlight = background(first);
    fireEvent.mouseEnter(second);

    // Menu keyboard navigation moves DOM focus without moving the pointer.
    act(() => first.focus());
    expect(background(first)).toBe(highlight);
    expect(background(second)).not.toBe(highlight);

    fireEvent.pointerMove(second);
    expect(document.activeElement).toBe(second);
    expect(background(second)).toBe(highlight);
    expect(background(first)).not.toBe(highlight);
  });

  it("does not move focus to a disabled item", () => {
    const { first, disabled } = renderMenu();
    fireEvent.mouseEnter(disabled);
    fireEvent.pointerMove(disabled);
    expect(document.activeElement).toBe(first);
    expect(background(disabled)).not.toBe(background(first));
  });
});
