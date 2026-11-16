import { isCompactBreakpoint } from "@/constants/layout";

/**
 * Density sizes for surfaces that opt into touch sizing: they grow on compact layouts and keep
 * their desktop sizes from the `md` breakpoint up. Plain theme tokens stay desktop-sized
 * everywhere, so a surface migrates by swapping its sizes for these.
 *
 * Style values are Unistyles breakpoint values, resolved inside `StyleSheet.create`.
 */

// Compact layouts end where `md` begins; see `isCompactBreakpoint`.
function byLayout<T>(compact: T, desktop: T) {
  return { xs: compact, md: desktop };
}

export const density = {
  /** The space between two blocks of rich text, such as Markdown paragraphs, code and images. */
  blockGap: byLayout(12, 16),
};

/** Resolves a density value for a prop, which Unistyles cannot resolve on its own. */
export function resolveDensity<T>(rt: { breakpoint?: string }, value: { xs: T; md: T }): T {
  return isCompactBreakpoint(rt.breakpoint) ? value.xs : value.md;
}
