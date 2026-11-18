/** @vitest-environment jsdom */
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { EMPTY_TERMINAL_KEY_MODIFIERS } from "@/terminal/runtime/terminal-key-dispatch";
import { TerminalKeyStrip, type TerminalKeyStripProps } from "./terminal-key-strip";

afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("en");
});

function renderStrip(overrides: Partial<TerminalKeyStripProps> = {}) {
  const props: TerminalKeyStripProps = {
    modifiers: EMPTY_TERMINAL_KEY_MODIFIERS,
    isKeyboardVisible: false,
    showPaste: true,
    hasClipboardText: true,
    onSendKey: vi.fn(),
    onToggleModifier: vi.fn(),
    onPaste: vi.fn(),
    onToggleKeyboard: vi.fn(),
    ...overrides,
  };
  return { props, view: render(<TerminalKeyStrip {...props} />) };
}

describe("terminal key strip", () => {
  it("sends the key behind each cap, including the icon-only ones", () => {
    const { props, view } = renderStrip();

    for (const id of ["esc", "tab", "up", "left", "down", "right", "enter"]) {
      fireEvent.click(view.getByTestId(`terminal-key-${id}`));
    }

    expect(vi.mocked(props.onSendKey).mock.calls.map(([key]) => key)).toEqual([
      "Escape",
      "Tab",
      "ArrowUp",
      "ArrowLeft",
      "ArrowDown",
      "ArrowRight",
      "Enter",
    ]);
  });

  it("names the icon-only caps for assistive technology", () => {
    const { view } = renderStrip();

    expect(
      ["up", "left", "down", "right", "enter"].map((id) =>
        view.getByTestId(`terminal-key-${id}`).getAttribute("aria-label"),
      ),
    ).toEqual(["↑", "←", "↓", "→", "Enter"]);
    expect(view.getByTestId("terminal-keyboard-toggle").getAttribute("aria-label")).toBe(
      "Show keyboard",
    );
    expect(view.getByTestId("terminal-paste").getAttribute("aria-label")).toBe("Paste");
  });

  it("marks only the latched modifier as pressed", () => {
    const { props, view } = renderStrip({
      modifiers: { ...EMPTY_TERMINAL_KEY_MODIFIERS, ctrl: true },
    });

    expect(
      ["ctrl", "alt", "shift"].map((modifier) =>
        view.getByTestId(`terminal-key-${modifier}`).getAttribute("aria-pressed"),
      ),
    ).toEqual(["true", "false", "false"]);

    expect(view.getByTestId("terminal-key-esc").hasAttribute("aria-pressed")).toBe(false);

    fireEvent.click(view.getByTestId("terminal-key-alt"));
    expect(props.onToggleModifier).toHaveBeenCalledWith("alt");
  });

  it("offers to hide the keyboard once it is showing", () => {
    const { props, view } = renderStrip({ isKeyboardVisible: true });
    const toggle = view.getByTestId("terminal-keyboard-toggle");

    expect(toggle.getAttribute("aria-label")).toBe("Hide keyboard");
    fireEvent.click(toggle);
    expect(props.onToggleKeyboard).toHaveBeenCalledTimes(1);
  });

  it("leaves Paste out where the platform cannot paste, and blocks it on an empty clipboard", () => {
    expect(renderStrip({ showPaste: false }).view.queryByTestId("terminal-paste")).toBeNull();
    cleanup();

    const { props, view } = renderStrip({ hasClipboardText: false });
    fireEvent.click(view.getByTestId("terminal-paste"));
    expect(props.onPaste).not.toHaveBeenCalled();
  });
});
