/** Puts the first logical option closest to the input, which sits below the list. */
export function orderAutocompleteOptions<T>(options: readonly T[]): T[] {
  return [...options].toReversed();
}

export function getAutocompleteFallbackIndex(itemCount: number): number {
  if (itemCount <= 0) {
    return -1;
  }
  return itemCount - 1;
}

export function getAutocompleteScrollOffset(args: {
  currentOffset: number;
  viewportHeight: number;
  itemTop: number;
  itemHeight: number;
}): number {
  if (args.viewportHeight <= 0) {
    return args.currentOffset;
  }

  const itemBottom = args.itemTop + args.itemHeight;
  const viewportTop = args.currentOffset;
  const viewportBottom = args.currentOffset + args.viewportHeight;

  if (args.itemTop < viewportTop) {
    return Math.max(0, args.itemTop);
  }

  if (itemBottom > viewportBottom) {
    return Math.max(0, itemBottom - args.viewportHeight);
  }

  return args.currentOffset;
}
