/**
 * How much of a workspace's CI a sidebar row spends space on.
 *
 * Not one of the boolean row items, because the icon on its own is a real third answer rather
 * than half of "off": the state stays on the row for a glance, and a passing workspace stops
 * paying a word for it on every line. A boolean cannot say that, and widening the row-items
 * record to hold it would make three settings carry a shape only one of them uses.
 *
 * Pure on purpose, like `row-items.ts`, so `hooks/use-settings/storage.ts` can import it.
 */

export const SIDEBAR_CHECKS_DISPLAYS = ["iconAndText", "icon", "none"] as const;

export type SidebarChecksDisplay = (typeof SIDEBAR_CHECKS_DISPLAYS)[number];

export const DEFAULT_SIDEBAR_CHECKS_DISPLAY: SidebarChecksDisplay = "iconAndText";
