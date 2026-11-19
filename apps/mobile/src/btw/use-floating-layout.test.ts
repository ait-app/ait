import { expect, it, vi } from "vitest";
import { btwFramePosition, clampBtwPosition, placeBtwFrame } from "./floating-drag";
import {
  btwKeyboardClearance,
  resolveBtwDragBounds,
  resolveBtwFloatingFrame,
  resolveBtwNativeLayout,
} from "./use-floating-layout";

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

const nativeInsets = { top: 44, bottom: 34, left: 0, right: 0 };
const modal = { width: 390, height: 820 };
const nativeAnchor = { x: 12, y: 705, width: 366, height: 72 };

it("keeps a docked frame above the composer after the composer has moved", () => {
  const shift = 280;
  const shiftedAnchor = { ...nativeAnchor, y: nativeAnchor.y - shift };
  const layout = resolveBtwNativeLayout(shiftedAnchor, modal, nativeInsets, shift);
  expect(layout.layoutHeight - layout.frame.bottom).toBe(shiftedAnchor.y - 12);
  expect(btwKeyboardClearance(layout.frame, nativeInsets.bottom)).toBeGreaterThan(shift);
  expect(layout.frame.height).toBe(361);
});

it("caps the native frame and drag box at the keyboard edge in a short dialog", () => {
  const size = { width: 700, height: 390 };
  const insets = { top: 0, bottom: 21, left: 44, right: 44 };
  const shift = 260;
  const layout = resolveBtwNativeLayout({ ...nativeAnchor, y: 330 }, size, insets, shift);

  expect(layout.layoutHeight).toBe(size.height);
  expect(layout.frame.height).toBe(93);
  expect(layout.dragBounds).toEqual({ minLeft: 52, maxLeft: 264, minTop: 8, maxTop: 8 });
  const overlap = shift - btwKeyboardClearance(layout.frame, insets.bottom);
  expect(btwFramePosition(layout.frame, size.height).top - overlap).toBe(layout.dragBounds.minTop);

  const shiftedAnchor = { ...nativeAnchor, y: 330 - shift };
  const shiftedLayout = resolveBtwNativeLayout(shiftedAnchor, size, insets, shift);
  expect(shiftedLayout.frame.height).toBe(93);
  expect(btwFramePosition(shiftedLayout.frame, size.height).top).toBe(8);
});

it("moves a native drag placement clear of the keyboard and back to its resting position", () => {
  const shifted = resolveBtwNativeLayout(nativeAnchor, modal, nativeInsets, 280);
  const resting = resolveBtwNativeLayout(nativeAnchor, modal, nativeInsets, 0);
  const saved = { left: 8, top: 345 };
  const placement = placeBtwFrame(
    shifted.frame,
    clampBtwPosition(saved, shifted.restingDragBounds),
    shifted.layoutHeight,
  );
  const overlap = 280 - btwKeyboardClearance(placement, nativeInsets.bottom);
  expect(saved.top - overlap).toBe(shifted.dragBounds.maxTop);
  expect(clampBtwPosition(saved, shifted.dragBounds).top).toBe(78);
  expect(clampBtwPosition(saved, resting.dragBounds)).toEqual(saved);
});
