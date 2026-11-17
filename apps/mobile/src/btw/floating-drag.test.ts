import { expect, it } from "vitest";
import {
  btwFramePosition,
  clampBtwPosition,
  placeBtwFrame,
  type BtwDragBounds,
  type BtwFloatingFrame,
} from "./floating-drag";

const bounds: BtwDragBounds = { minLeft: 8, maxLeft: 1048, minTop: 8, maxTop: 472 };
const frame: BtwFloatingFrame = { left: 1048, bottom: 144, width: 384, height: 420 };

it("round-trips between the composer's bottom-anchored frame and screen coordinates", () => {
  const position = btwFramePosition(frame, 900);
  expect(position).toEqual({ left: 1048, top: 336 });
  expect(placeBtwFrame(frame, position, 900)).toEqual(frame);
});

it("keeps the window inside its box at every edge and after a resize", () => {
  expect(clampBtwPosition({ left: -5000, top: -5000 }, bounds)).toEqual({ left: 8, top: 8 });
  expect(clampBtwPosition({ left: 5000, top: 5000 }, bounds)).toEqual({ left: 1048, top: 472 });
  expect(clampBtwPosition({ left: 400, top: 200 }, bounds)).toEqual({ left: 400, top: 200 });
});
