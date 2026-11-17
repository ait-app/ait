import { expect, it, vi } from "vitest";
import { resolveBtwDragBounds, resolveBtwFloatingFrame } from "./use-floating-layout";

vi.mock("@/keyboard/shift", () => ({ useSettledKeyboardShift: () => 0 }));

it("hugs the window's right edge with a gap above its composer", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 470, y: 768, width: 820, height: 120 },
      { x: 0, y: 0, width: 1440, height: 900, layoutHeight: 900 },
      0,
    ),
  ).toEqual({ left: 1048, bottom: 144, width: 384, height: 420 });
});

it("caps the window size in a wide viewport and shrinks it in a narrow one", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 900, y: 480, width: 300, height: 180 },
      { x: 0, y: 0, width: 1440, height: 700, layoutHeight: 700 },
      0,
    ),
  ).toEqual({ left: 1048, bottom: 232, width: 384, height: 420 });
  expect(
    resolveBtwFloatingFrame(
      { x: 16, y: 200, width: 374, height: 176 },
      { x: 0, y: 0, width: 390, height: 400, layoutHeight: 400 },
      0,
    ),
  ).toEqual({ left: 8, bottom: 212, width: 374, height: 180 });
});

it("respects the visible viewport's pan and the safe top inset", () => {
  expect(
    resolveBtwFloatingFrame(
      { x: 196, y: 330, width: 358, height: 120 },
      { x: 180, y: 100, width: 390, height: 300, layoutHeight: 900 },
      22,
    ),
  ).toEqual({ left: 188, bottom: 582, width: 374, height: 188 });
});

it("lets a dragged window travel to every edge of the viewport", () => {
  expect(
    resolveBtwDragBounds({ x: 0, y: 0, width: 1440, height: 900, layoutHeight: 900 }, 0, {
      width: 384,
      height: 420,
    }),
  ).toEqual({ minLeft: 8, maxLeft: 1048, minTop: 8, maxTop: 472 });
  expect(
    resolveBtwDragBounds({ x: 180, y: 100, width: 390, height: 300, layoutHeight: 900 }, 22, {
      width: 374,
      height: 188,
    }),
  ).toEqual({ minLeft: 188, maxLeft: 188, minTop: 130, maxTop: 204 });
});
