/** Floating UI's native clipping rect includes the area covered by the keyboard. */
export function nativeComboboxCollisionPadding(input: {
  windowHeight: number;
  keyboardTop: number | null;
  padding: number;
}) {
  const visibleBottom = Math.max(
    0,
    Math.min(input.windowHeight, input.keyboardTop ?? input.windowHeight),
  );
  return {
    top: input.padding,
    left: input.padding,
    right: input.padding,
    bottom: input.windowHeight - visibleBottom + input.padding,
  };
}
