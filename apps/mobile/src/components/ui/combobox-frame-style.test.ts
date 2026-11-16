import type { ViewStyle } from "react-native";
import { describe, expect, it } from "vitest";

import { buildDesktopFrameStyle } from "./combobox-frame-style";

function buildWidthStyle(input: {
  desktopMinWidth?: number;
  referenceWidth: number | null;
}): Pick<ViewStyle, "width" | "minWidth" | "maxWidth"> {
  const [frameStyle] = buildDesktopFrameStyle({
    desktopMinWidth: input.desktopMinWidth,
    desktopLockWidth: false,
    referenceWidth: input.referenceWidth,
    desktopFixedHeight: undefined,
    desktopPositionStyle: { left: 0, top: 0 },
    shouldHideDesktopContent: false,
    availableHeight: undefined,
  }) as ViewStyle[];

  return {
    width: frameStyle.width,
    minWidth: frameStyle.minWidth,
    maxWidth: frameStyle.maxWidth,
  };
}

describe("buildDesktopFrameStyle", () => {
  it("lets a narrow trigger grow to the default desktop ceiling", () => {
    expect(buildWidthStyle({ referenceWidth: 120 })).toEqual({
      width: undefined,
      minWidth: 120,
      maxWidth: 400,
    });
  });

  it("keeps a wide trigger from being capped below its own width", () => {
    expect(buildWidthStyle({ referenceWidth: 470 })).toEqual({
      width: undefined,
      minWidth: 470,
      maxWidth: 470,
    });
  });

  it("uses desktopMinWidth as an explicit floor raiser", () => {
    expect(buildWidthStyle({ desktopMinWidth: 360, referenceWidth: 120 })).toEqual({
      width: undefined,
      minWidth: 360,
      maxWidth: 400,
    });
  });

  it("keeps the trigger as the floor when it is wider than desktopMinWidth", () => {
    expect(buildWidthStyle({ desktopMinWidth: 240, referenceWidth: 300 })).toEqual({
      width: undefined,
      minWidth: 300,
      maxWidth: 400,
    });
  });

  it("locks the frame to its opening floor when content must not resize it", () => {
    const [frameStyle] = buildDesktopFrameStyle({
      desktopMinWidth: 360,
      desktopLockWidth: true,
      referenceWidth: 120,
      desktopFixedHeight: undefined,
      desktopPositionStyle: { left: 0, top: 0 },
      shouldHideDesktopContent: false,
      availableHeight: undefined,
    }) as ViewStyle[];

    expect(frameStyle).toMatchObject({ width: 360, minWidth: 360, maxWidth: 360 });
  });
});

describe("native popover widths", () => {
  function nativeWidth(input: {
    referenceWidth?: number;
    desktopMinWidth?: number;
    availableWidth?: number;
    desktopLockWidth?: boolean;
  }) {
    const [frame] = buildDesktopFrameStyle({
      desktopMinWidth: input.desktopMinWidth,
      desktopLockWidth: input.desktopLockWidth ?? false,
      referenceWidth: input.referenceWidth ?? 85,
      desktopFixedHeight: undefined,
      desktopPositionStyle: { left: 0, top: 0 },
      shouldHideDesktopContent: false,
      availableHeight: undefined,
      nativePopover: true,
      availableWidth: input.availableWidth ?? 700,
    }) as ViewStyle[];
    return { width: frame.width, minWidth: frame.minWidth, maxWidth: frame.maxWidth };
  }

  it("gives short checkout badge menus a readable search and option width", () => {
    expect(nativeWidth({})).toEqual({ width: 240, minWidth: 240, maxWidth: 240 });
  });

  it("preserves wider anchors and explicit picker minimums", () => {
    expect(nativeWidth({ referenceWidth: 300 }).width).toBe(300);
    expect(nativeWidth({ desktopMinWidth: 360 }).width).toBe(360);
    expect(nativeWidth({ desktopMinWidth: 200 }).width).toBe(200);
  });

  it("clamps even locked or oversized menus to the available display width", () => {
    expect(
      nativeWidth({ desktopMinWidth: 360, availableWidth: 280, desktopLockWidth: true }),
    ).toEqual({ width: 280, minWidth: 280, maxWidth: 280 });
    expect(nativeWidth({ referenceWidth: 470, availableWidth: 440 }).width).toBe(440);
    expect(nativeWidth({ availableWidth: 160 }).width).toBe(160);
  });
});
