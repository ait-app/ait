import {
  TERMINAL_VIRTUAL_KEY_BUTTONS,
  type TerminalKeyModifierState,
} from "./terminal-key-dispatch";

export type TerminalVirtualKeyButton =
  (typeof TERMINAL_VIRTUAL_KEY_BUTTONS)[keyof typeof TERMINAL_VIRTUAL_KEY_BUTTONS];

export type TerminalVirtualKeyboardControl =
  | {
      type: "key";
      button: TerminalVirtualKeyButton;
    }
  | {
      type: "modifier";
      modifier: keyof TerminalKeyModifierState;
    }
  | {
      type: "paste";
    }
  | {
      type: "keyboardToggle";
    };

// A gap holds a column open without a control in it, so the arrow above the
// bottom row stays over the down arrow.
export type TerminalVirtualKeyboardSlot =
  | TerminalVirtualKeyboardControl
  | {
      type: "gap";
      id: string;
    };

export type TerminalVirtualKeyboardClusterId = "keys" | "arrows" | "actions";

interface TerminalVirtualKeyboardCluster {
  id: TerminalVirtualKeyboardClusterId;
  rows: readonly (readonly TerminalVirtualKeyboardSlot[])[];
}

// Three clusters, each two rows tall: the named keys over the modifiers they
// combine with, the arrows as an inverted T, and the app actions over Enter.
export const TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS = [
  {
    id: "keys",
    rows: [
      [
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.esc },
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.tab },
      ],
      [
        { type: "modifier", modifier: "ctrl" },
        { type: "modifier", modifier: "alt" },
        { type: "modifier", modifier: "shift" },
      ],
    ],
  },
  {
    id: "arrows",
    rows: [
      [
        { type: "gap", id: "arrows-gap-start" },
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.up },
        { type: "gap", id: "arrows-gap-end" },
      ],
      [
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.left },
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.down },
        { type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.right },
      ],
    ],
  },
  {
    id: "actions",
    rows: [
      [{ type: "paste" }, { type: "keyboardToggle" }],
      [{ type: "key", button: TERMINAL_VIRTUAL_KEY_BUTTONS.enter }],
    ],
  },
] as const satisfies readonly TerminalVirtualKeyboardCluster[];

export function getTerminalVirtualKeyboardSlotId(slot: TerminalVirtualKeyboardSlot): string {
  switch (slot.type) {
    case "key":
      return `terminal-key-${slot.button.id}`;
    case "modifier":
      return `terminal-key-${slot.modifier}`;
    case "paste":
      return "terminal-paste";
    case "keyboardToggle":
      return "terminal-keyboard-toggle";
    case "gap":
      return slot.id;
  }
}

export function shouldShowTerminalPasteAction(input: { isNative: boolean }): boolean {
  return input.isNative;
}

export function shouldShowTerminalFloatingCopyAction(input: {
  hasSelection: boolean;
  isNative: boolean;
}): boolean {
  return input.isNative && input.hasSelection;
}
