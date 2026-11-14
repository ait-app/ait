import { describe, expect, it } from "vitest";
import { formatShortcut } from "@/utils/format-shortcut";
import {
  comboStringToShortcutKeys,
  keyboardEventToComboString,
  parseShortcutString,
  SHORTCUT_KEY_NAMES,
} from "./shortcut-string";

function keyboardEvent(overrides: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "",
    code: "",
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    repeat: false,
    ...overrides,
  } as KeyboardEvent;
}

describe("keyboardEventToComboString", () => {
  it("captures the minus key (regression: previously returned null)", () => {
    expect(keyboardEventToComboString(keyboardEvent({ key: "-", code: "Minus" }))).toBe("-");
  });

  it("captures minus with modifiers", () => {
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "-", code: "Minus", metaKey: true })),
    ).toBe("Cmd+-");
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "_", code: "Minus", shiftKey: true })),
    ).toBe("Shift+-");
  });

  it("captures equal, semicolon, and quote", () => {
    expect(keyboardEventToComboString(keyboardEvent({ key: "=", code: "Equal" }))).toBe("=");
    expect(keyboardEventToComboString(keyboardEvent({ key: ";", code: "Semicolon" }))).toBe(";");
    expect(keyboardEventToComboString(keyboardEvent({ key: "'", code: "Quote" }))).toBe("'");
  });

  it("records a printable key by the character it produced, not its physical position", () => {
    // Dvorak: the key labelled "T" sits on physical KeyK.
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "t", code: "KeyK", metaKey: true })),
    ).toBe("Cmd+T");
    // Dvorak: "v" sits on physical Period.
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "v", code: "Period", ctrlKey: true })),
    ).toBe("Ctrl+V");
    // German: Shift+7 produces "/".
    expect(
      keyboardEventToComboString(
        keyboardEvent({ key: "/", code: "Digit7", shiftKey: true, metaKey: true }),
      ),
    ).toBe("Shift+Cmd+/");
  });

  it("records shifted characters as Shift plus the base key, as QWERTY did before", () => {
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "!", code: "Digit1", shiftKey: true })),
    ).toBe("Shift+1");
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "?", code: "Slash", shiftKey: true })),
    ).toBe("Shift+/");
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "T", code: "KeyT", shiftKey: true })),
    ).toBe("Shift+T");
  });

  it("records Alt combos and unrepresentable characters by physical key", () => {
    // macOS rewrites event.key under Option; the matcher compares Alt bindings by code.
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "†", code: "KeyT", altKey: true })),
    ).toBe("Alt+T");
    expect(
      keyboardEventToComboString(keyboardEvent({ key: "л", code: "KeyK", ctrlKey: true })),
    ).toBe("Ctrl+K");
    expect(keyboardEventToComboString(keyboardEvent({ key: " ", code: "Space" }))).toBe("Space");
    expect(keyboardEventToComboString(keyboardEvent({ key: "Enter", code: "Enter" }))).toBe(
      "Enter",
    );
  });

  it("still returns null for modifier-only presses", () => {
    expect(keyboardEventToComboString(keyboardEvent({ code: "ShiftLeft" }))).toBeNull();
  });
});

describe("parseShortcutString parses punctuation keys", () => {
  const cases: ReadonlyArray<[string, string, string, string]> = [
    ["-", "Minus", "-", "_"],
    ["=", "Equal", "=", "+"],
    [";", "Semicolon", ";", ":"],
    ["'", "Quote", "'", '"'],
  ];

  for (const [humanKey, code, key, shiftedKey] of cases) {
    it(`parses ${humanKey}`, () => {
      const combo = parseShortcutString(humanKey);
      expect(combo.code).toBe(code);
      expect(combo.key).toBe(key);
      expect(combo.shiftedKey).toBe(shiftedKey);
    });
  }

  it("parses a modifier combo with minus", () => {
    const combo = parseShortcutString("Cmd+-");
    expect(combo.meta).toBe(true);
    expect(combo.code).toBe("Minus");
  });
});

describe("comboStringToShortcutKeys", () => {
  it("maps every modifier spelling onto its display token", () => {
    expect(comboStringToShortcutKeys("Cmd+Ctrl+Alt+Shift+K")).toEqual([
      "mod",
      "ctrl",
      "alt",
      "shift",
      "K",
    ]);
    // `parseShortcutString` accepts `Mod` as well, so the display path has to.
    expect(comboStringToShortcutKeys("Mod+K")).toEqual(["mod", "K"]);
  });

  it("hands named keys to the display table under the names it uses", () => {
    expect(comboStringToShortcutKeys("Cmd+Shift+ArrowLeft")).toEqual(["mod", "shift", "Left"]);
    expect(formatShortcut(comboStringToShortcutKeys("Cmd+Shift+ArrowLeft"), "mac")).toBe("⇧⌘←");
    expect(formatShortcut(comboStringToShortcutKeys("Escape"), "mac")).toBe("Esc");
    expect(formatShortcut(comboStringToShortcutKeys("Backspace"), "mac")).toBe("⌫");
  });

  // The whole class, not just the arrows: uppercasing key names used to push
  // every multi-character name past `KEY_DISPLAY`, so an override on one of them
  // rendered as a shouty raw code — `ARROWLEFT`, `ESCAPE`, `PAGEDOWN`.
  it("never emits a shouty raw code for any key the capture path can produce", () => {
    for (const name of SHORTCUT_KEY_NAMES) {
      // `F1` and the digits are already all-caps, so only names that contain a
      // lowercase letter can exhibit the bug.
      if (name === name.toUpperCase()) continue;
      const [key] = comboStringToShortcutKeys(name);
      const rendered = formatShortcut([key], "non-mac");
      expect(rendered, `${name} renders as ${rendered}`).not.toBe(name.toUpperCase());
    }
  });
});
