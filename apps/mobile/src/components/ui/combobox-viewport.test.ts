import { describe, expect, it } from "vitest";
import { computePosition, flip, offset, shift, size } from "@floating-ui/core";
import { nativeComboboxCollisionPadding } from "./combobox-viewport";

describe("native combobox keyboard viewport", () => {
  it("excludes the keyboard without subtracting it twice from resized windows", () => {
    expect(
      nativeComboboxCollisionPadding({ windowHeight: 880, keyboardTop: 540, padding: 16 }).bottom,
    ).toBe(356);
    expect(
      nativeComboboxCollisionPadding({ windowHeight: 540, keyboardTop: 540, padding: 16 }).bottom,
    ).toBe(16);
    expect(
      nativeComboboxCollisionPadding({ windowHeight: 880, keyboardTop: null, padding: 16 }).bottom,
    ).toBe(16);
  });

  it("repositions the search menu above its moved anchor and keeps rows above the keyboard", async () => {
    const padding = nativeComboboxCollisionPadding({
      windowHeight: 880,
      keyboardTop: 540,
      padding: 16,
    });
    let availableHeight = 0;
    const result = await computePosition(
      {},
      {},
      {
        placement: "bottom-start",
        middleware: [
          offset(4),
          flip({ padding }),
          shift({ padding }),
          size({
            padding,
            apply: (state) => {
              availableHeight = state.availableHeight;
            },
          }),
        ],
        platform: {
          getElementRects: async () => ({
            reference: { x: 550, y: 375, width: 85, height: 32 },
            floating: { x: 0, y: 0, width: 240, height: 190 },
          }),
          getClippingRect: async () => ({ x: 0, y: 0, width: 852, height: 880 }),
          getDimensions: async () => ({ width: 240, height: 190 }),
        },
      },
    );
    expect(result.placement).toBe("top-start");
    expect(result.y + 190).toBeLessThanOrEqual(540 - 16);
    expect(result.y).toBeGreaterThanOrEqual(16);
    expect(availableHeight).toBeGreaterThanOrEqual(190);
  });
});
