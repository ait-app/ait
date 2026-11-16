/**
 * Which facts a sidebar workspace row is allowed to show about a workspace.
 *
 * Each item is independent — this is not a mode. Turning one off removes it from every row and
 * gives the space back to the title.
 *
 * CI is not here: it has three answers rather than two, so it is its own setting in
 * `checks-display.ts`.
 *
 * Pure on purpose, so `hooks/use-settings/storage.ts` can import it.
 */

export const SIDEBAR_ROW_ITEMS = [
  "branch",
  "project",
  "host",
  "changeRequest",
  "services",
  "labels",
] as const;

export type SidebarRowItem = (typeof SIDEBAR_ROW_ITEMS)[number];

export type SidebarRowItems = Record<SidebarRowItem, boolean>;

/** The persisted record is merged over these explicit product defaults. */
export const DEFAULT_SIDEBAR_ROW_ITEMS: SidebarRowItems = {
  branch: false,
  project: false,
  host: true,
  changeRequest: true,
  services: true,
  labels: true,
};

/**
 * COMPAT(sidebarRowItemsChecks): CI was a boolean row item until v0.3.0, when it became a
 * display mode of its own. A stored `false` is someone who switched checks off, and they must
 * not come back as icon-and-text on upgrade. Remove after 2027-08-05.
 */
export function isChecksHiddenByLegacyRowItem(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  return (value as Record<string, unknown>).checks === false;
}
