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

/** Which groups the card renders, in display order. */
export interface ModelCardSections {
  /** The provider's own models, shown under "Default model". */
  defaultModel: boolean;
  /** The user's saved configurations, shown under "Model presets". */
  modelPresets: boolean;
}

/**
 * Resolve the groups for one provider.
 *
 * `modelCount` is the number of rows the provider actually offers and `presetCount` the saved
 * configurations that belong to it. An error replaces the model list, so the heading is withheld
 * rather than naming an empty group; the preset group keeps its own visibility rule.
 */
export function resolveModelCardSections(input: {
  modelCount: number;
  presetCount: number;
  modelError: boolean;
}): ModelCardSections {
  return {
    defaultModel: !input.modelError && input.modelCount > 0,
    modelPresets: input.presetCount > 0,
  };
}
