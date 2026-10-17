import { describe, expect, it } from "vitest";
import {
  resolveNativePaneDropTarget,
  type NativePaneFrame,
} from "@/components/native-pane-drop-target";

const panes: NativePaneFrame[] = [
  { paneId: "left", left: 0, top: 0, width: 400, height: 800 },
  { paneId: "right", left: 400, top: 0, width: 400, height: 800 },
];

describe("resolveNativePaneDropTarget", () => {
  it("resolves a side split from the target pane edges", () => {
    expect(resolveNativePaneDropTarget({ x: 430, y: 400, panes })).toEqual({
      paneId: "right",
      position: "left",
    });
    expect(resolveNativePaneDropTarget({ x: 770, y: 400, panes })).toEqual({
      paneId: "right",
      position: "right",
    });
  });

  it("returns the center target and ignores points outside visible pane frames", () => {
    expect(resolveNativePaneDropTarget({ x: 600, y: 400, panes })).toEqual({
      paneId: "right",
      position: "center",
    });
    expect(resolveNativePaneDropTarget({ x: 900, y: 400, panes })).toBeNull();
  });
});
