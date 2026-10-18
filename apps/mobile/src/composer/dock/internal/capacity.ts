interface ComposerGeometry {
  height: number;
  bottomInset: number;
  keyboardShift: number;
  centered: boolean;
}

export interface ComposerCapacity {
  keyboardReserve: number;
  capacity: number;
}

// The empty composer at its tallest breakpoint: border, padding, one input line,
// the gap and the button row. A tighter bound would clip the controls, and
// Android drops touches outside a parent's bounds, so the send button stops working.
export const MIN_COMPOSER_CAPACITY = 2 + 16 + 30 + 12 + 28 + 12;

export function resolveComposerCapacity(input: ComposerGeometry): number {
  "worklet";
  // A centered form grows upward by half its height. Reserve both halves so
  // translating it still leaves five layout points below the header.
  const clearance = input.keyboardShift + 5;
  const reservedSpace = input.centered ? clearance * 2 : clearance;
  // A tall keyboard on a short viewport (a foldable's square inner screen) leaves
  // less than the composer itself. Keep the composer usable; the form may then
  // overlap the header slightly while the keyboard is open.
  return Math.max(MIN_COMPOSER_CAPACITY, input.height - input.bottomInset - reservedSpace);
}

export function updateComposerCapacity(
  previous: ComposerCapacity | undefined,
  input: ComposerGeometry,
): ComposerCapacity {
  "worklet";
  if (input.height <= 0 && previous) return previous;
  // Closing the keyboard moves the composer; it does not enlarge its editing
  // capacity. A subsequent keyboard opening supplies the next reservation.
  const keyboardReserve =
    input.keyboardShift > 0 ? input.keyboardShift : (previous?.keyboardReserve ?? 0);
  return {
    keyboardReserve,
    capacity: resolveComposerCapacity({ ...input, keyboardShift: keyboardReserve }),
  };
}
