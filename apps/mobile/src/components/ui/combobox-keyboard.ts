export function getNextActiveIndex(args: {
  currentIndex: number;
  itemCount: number;
  key: "ArrowDown" | "ArrowUp";
}): number {
  const { currentIndex, itemCount, key } = args;

  if (itemCount <= 0) return -1;

  if (currentIndex < 0) {
    return key === "ArrowDown" ? 0 : itemCount - 1;
  }

  const normalizedCurrent = currentIndex % itemCount;
  return key === "ArrowDown"
    ? (normalizedCurrent + 1) % itemCount
    : (normalizedCurrent - 1 + itemCount) % itemCount;
}

/** Menu navigation yields to text editing and IME; only its own search uses arrow navigation. */
export function shouldHandleComboboxKey(event: KeyboardEvent): boolean {
  if (event.isComposing || event.keyCode === 229) return false;
  if (event.key === "Escape") return true;
  const target = event.target;
  if (!(target instanceof Element)) return true;
  const editor = target.closest('input, textarea, [contenteditable="true"], [role="textbox"]');
  return !editor || Boolean(target.closest("[data-combobox-search]"));
}
