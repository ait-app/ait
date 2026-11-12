import type { StyleProp, ViewStyle } from "react-native";

export interface DesktopFrameStyleInput {
  desktopMinWidth: number | undefined;
  desktopLockWidth: boolean;
  referenceWidth: number | null;
  desktopFixedHeight: number | undefined;
  desktopPositionStyle: StyleProp<ViewStyle>;
  shouldHideDesktopContent: boolean;
  availableHeight: number | undefined;
  nativePopover?: boolean;
  availableWidth?: number;
}

export function buildDesktopFrameStyle(input: DesktopFrameStyleInput): StyleProp<ViewStyle> {
  const {
    desktopMinWidth,
    desktopLockWidth,
    referenceWidth,
    desktopFixedHeight,
    desktopPositionStyle,
    shouldHideDesktopContent,
    availableHeight,
  } = input;
  const fixedHeightStyle =
    desktopFixedHeight != null
      ? { minHeight: desktopFixedHeight, maxHeight: desktopFixedHeight }
      : null;
  const hiddenStyle = shouldHideDesktopContent ? { opacity: 0 } : null;
  const availableHeightStyle =
    typeof availableHeight === "number"
      ? { maxHeight: Math.min(availableHeight, desktopFixedHeight ?? 400) }
      : null;
  const floor = Math.max(desktopMinWidth ?? 0, referenceWidth ?? 200);
  // Native flex rows do not provide an intrinsic popover width. In particular,
  // a short badge anchor must not collapse the search field and option labels.
  const nativeWidth = Math.min(
    Math.max(desktopMinWidth ?? 240, referenceWidth ?? 0),
    Math.max(0, input.availableWidth ?? Number.POSITIVE_INFINITY),
  );
  const widthStyle = input.nativePopover
    ? { width: nativeWidth, minWidth: nativeWidth, maxWidth: nativeWidth }
    : desktopLockWidth
      ? { width: floor, minWidth: floor, maxWidth: floor }
      : { minWidth: floor, maxWidth: Math.max(400, floor) };
  return [
    {
      position: "absolute" as const,
      ...widthStyle,
    },
    fixedHeightStyle,
    desktopPositionStyle,
    hiddenStyle,
    availableHeightStyle,
  ];
}
