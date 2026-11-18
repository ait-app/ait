import { describe, expect, it } from "vitest";

import {
  getTerminalVirtualKeyboardSlotId,
  shouldShowTerminalFloatingCopyAction,
  shouldShowTerminalPasteAction,
  TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS,
  type TerminalVirtualKeyboardSlot,
} from "./terminal-virtual-keyboard";

interface ControlPosition {
  cluster: string;
  row: number;
  col: number;
}

function slots(): TerminalVirtualKeyboardSlot[] {
  return TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS.flatMap((cluster) =>
    cluster.rows.flatMap((row): TerminalVirtualKeyboardSlot[] => [...row]),
  );
}

function controlIds(): string[] {
  return slots()
    .filter((slot) => slot.type !== "gap")
    .map((slot) => getTerminalVirtualKeyboardSlotId(slot));
}

function controlPositions(): Map<string, ControlPosition> {
  const positions = new Map<string, ControlPosition>();
  for (const cluster of TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS) {
    cluster.rows.forEach((row, rowIndex) => {
      row.forEach((slot, colIndex) => {
        positions.set(getTerminalVirtualKeyboardSlotId(slot), {
          cluster: cluster.id,
          row: rowIndex,
          col: colIndex,
        });
      });
    });
  }
  return positions;
}

function controlsByType(type: TerminalVirtualKeyboardSlot["type"]): TerminalVirtualKeyboardSlot[] {
  return slots().filter((slot) => slot.type === type);
}

describe("terminal virtual keyboard policy", () => {
  it("does not expose redundant Space or Backspace controls", () => {
    expect(controlIds()).toEqual([
      "terminal-key-esc",
      "terminal-key-tab",
      "terminal-key-ctrl",
      "terminal-key-alt",
      "terminal-key-shift",
      "terminal-key-up",
      "terminal-key-left",
      "terminal-key-down",
      "terminal-key-right",
      "terminal-paste",
      "terminal-keyboard-toggle",
      "terminal-key-enter",
    ]);
  });

  it("keeps the arrows in an inverted-T cluster of their own", () => {
    const positions = controlPositions();

    expect({
      up: positions.get("terminal-key-up"),
      left: positions.get("terminal-key-left"),
      down: positions.get("terminal-key-down"),
      right: positions.get("terminal-key-right"),
    }).toEqual({
      up: { cluster: "arrows", row: 0, col: 1 },
      left: { cluster: "arrows", row: 1, col: 0 },
      down: { cluster: "arrows", row: 1, col: 1 },
      right: { cluster: "arrows", row: 1, col: 2 },
    });
    expect(
      [...positions.values()].filter((position) => position.cluster === "arrows"),
    ).toHaveLength(6);
  });

  it("gives every cluster two rows so the strip keeps one height", () => {
    expect(TERMINAL_VIRTUAL_KEYBOARD_CLUSTERS.map((cluster) => cluster.rows.length)).toEqual([
      2, 2, 2,
    ]);
  });

  it("puts the modifiers together under the keys they combine with", () => {
    const positions = controlPositions();

    expect(
      ["ctrl", "alt", "shift"].map((modifier) => positions.get(`terminal-key-${modifier}`)),
    ).toEqual([
      { cluster: "keys", row: 1, col: 0 },
      { cluster: "keys", row: 1, col: 1 },
      { cluster: "keys", row: 1, col: 2 },
    ]);
  });

  it("keeps Paste in the keyboard and Copy out of the permanent row", () => {
    expect(controlsByType("paste")).toHaveLength(1);
    expect(controlIds()).toContain("terminal-paste");
    expect(controlIds()).not.toContain("terminal-copy");
  });

  it("shows Copy only as a native selection affordance", () => {
    expect(
      shouldShowTerminalFloatingCopyAction({
        hasSelection: false,
        isNative: true,
      }),
    ).toBe(false);
    expect(
      shouldShowTerminalFloatingCopyAction({
        hasSelection: true,
        isNative: false,
      }),
    ).toBe(false);
    expect(
      shouldShowTerminalFloatingCopyAction({
        hasSelection: true,
        isNative: true,
      }),
    ).toBe(true);
  });

  it("keeps Paste native-gated", () => {
    expect(shouldShowTerminalPasteAction({ isNative: true })).toBe(true);
    expect(shouldShowTerminalPasteAction({ isNative: false })).toBe(false);
  });
});
