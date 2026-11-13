import { expect, it, vi } from "vitest";
import { resolveBtwFloatingFrame } from "./use-floating-layout";

vi.mock("@/keyboard/shift", () => ({ useSettledKeyboardShift: () => 0 }));

it("aligns the window to its composer's right edge with an Ait gap above it", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 470, y: 768, width: 820, height: 120 },
      { x: 0, y: 0, width: 1440, height: 900, layoutHeight: 900 },
      0,
    ),
  ).toEqual({ left: 842, bottom: 144, width: 448, height: 560 });
});

it("stays inside a narrow pane instead of extending into the neighbouring pane", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 900, y: 480, width: 300, height: 180 },
      { x: 0, y: 0, width: 1440, height: 700, layoutHeight: 700 },
      0,
    ),
  ).toEqual({ left: 900, bottom: 232, width: 300, height: 460 });
});

it("shrinks below the former minimum height without crossing the composer or top edge", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 16, y: 200, width: 358, height: 176 },
      { x: 0, y: 0, width: 390, height: 400, layoutHeight: 400 },
      0,
    ),
  ).toEqual({ left: 16, bottom: 212, width: 358, height: 180 });
});

it("respects the visible viewport's pan and the safe top inset", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 196, y: 330, width: 358, height: 120 },
      { x: 180, y: 100, width: 390, height: 300, layoutHeight: 900 },
      22,
    ),
  ).toEqual({ left: 196, bottom: 582, width: 358, height: 188 });
});
