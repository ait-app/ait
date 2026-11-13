/**
 * Presentation rules for the model card's two groups.
 *
 * The card lists the provider's own models under "Default model" and the user's saved
 * configurations under "Model presets". A provider with no saved presets gets no preset group at
 * all, because managing them belongs to the provider's settings rather than every model card.
 */

/** Whether the preset group has anything to show for this provider. */
export function presetSectionIsVisible(rows: readonly unknown[]): boolean {
  return rows.length > 0;
}
