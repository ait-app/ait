import {
  filterAndRankModelRows,
  getAllProviderModelRows,
  getProviderModelRows,
  type ProviderSelectionModelRow,
  type ProviderSelectorProvider,
} from "@/provider-selection/provider-selection";

export type ModelBrowserView =
  | { kind: "all" }
  | { kind: "provider"; providerId: string; providerLabel: string };

export function resolveModelBrowserScrolling({
  isNative,
  isCompact,
}: {
  isNative: boolean;
  isCompact: boolean;
}): "sheet" | "independent" {
  return isNative && isCompact ? "sheet" : "independent";
}

/** A profile's model reference; used to match profiles back to model rows. */
export interface ModelProfileRef {
  provider: string;
  modelId: string;
}

/**
 * Groups profiles by `provider:modelId`, skipping profiles that name no model.
 * Pure so the model browser can test it apart from the component tree.
 */
export function groupProfilesByProviderModel<T extends ModelProfileRef>(
  refs: readonly T[],
): Map<string, T[]> {
  const lookup = new Map<string, T[]>();
  for (const ref of refs) {
    const modelId = ref.modelId.trim();
    if (!modelId) {
      continue;
    }
    const key = `${ref.provider}:${modelId}`;
    const existing = lookup.get(key);
    if (existing) {
      existing.push(ref);
    } else {
      lookup.set(key, [ref]);
    }
  }
  return lookup;
}

/** What the root view shows: the provider drill-down, or ranked cross-provider results. */
export type ModelBrowserAllView =
  | { kind: "browse" }
  | { kind: "searchResults"; rows: ProviderSelectionModelRow[] }
  | { kind: "noSearchMatches" };

export function resolveModelBrowserAllView({
  providers,
  normalizedQuery,
  isSearchFocused,
}: {
  providers: ProviderSelectorProvider[];
  normalizedQuery: string;
  isSearchFocused: boolean;
}): ModelBrowserAllView {
  if (!normalizedQuery && !isSearchFocused) {
    return { kind: "browse" };
  }
  const allRows = getAllProviderModelRows(providers);
  const rows = normalizedQuery ? filterAndRankModelRows(allRows, normalizedQuery) : allRows;
  if (rows.length === 0) {
    return { kind: "noSearchMatches" };
  }
  return { kind: "searchResults", rows };
}

/** Where the picker lands when it opens. A sole provider skips the redundant root view. */
export function resolveInitialModelBrowserView({
  providers,
  selectedProvider,
  selectedModel,
  hasProfiles,
}: {
  providers: ProviderSelectorProvider[];
  selectedProvider: string;
  selectedModel: string;
  hasProfiles: boolean;
}): ModelBrowserView {
  const singleProvider = providers.length === 1 ? providers[0] : undefined;
  if (singleProvider) {
    return {
      kind: "provider",
      providerId: singleProvider.id,
      providerLabel: singleProvider.label,
    };
  }

  if (hasProfiles) {
    return { kind: "all" };
  }

  if (selectedProvider.length > 0 && selectedModel.length > 0) {
    const provider = providers.find((entry) => entry.id === selectedProvider);
    if (provider) {
      return { kind: "provider", providerId: provider.id, providerLabel: provider.label };
    }
  }

  return { kind: "all" };
}

const DESKTOP_MIN_HEIGHT = 220;
const DESKTOP_MAX_HEIGHT = 400;
const DESKTOP_BASE_HEIGHT = 80;
const DESKTOP_ROW_HEIGHT = 40;

/**
 * Fixed desktop popover height, sized to the rows the current view renders.
 * The root browse view lists providers (plus the profiles section), not every
 * model, so it must not grow with the total model count.
 */
export function resolveDesktopFixedHeight({
  view,
  providers,
  profileRowCount,
  isSearching,
}: {
  view: ModelBrowserView;
  providers: ProviderSelectorProvider[];
  /** Saved profile rows, or null when the picker has no profiles section. */
  profileRowCount: number | null;
  isSearching: boolean;
}): number {
  const rowCount = countDesktopRowsInView({ view, providers, profileRowCount, isSearching });
  return Math.min(
    Math.max(DESKTOP_MIN_HEIGHT, DESKTOP_BASE_HEIGHT + rowCount * DESKTOP_ROW_HEIGHT),
    DESKTOP_MAX_HEIGHT,
  );
}

function countDesktopRowsInView({
  view,
  providers,
  profileRowCount,
  isSearching,
}: {
  view: ModelBrowserView;
  providers: ProviderSelectorProvider[];
  profileRowCount: number | null;
  isSearching: boolean;
}): number {
  if (view.kind === "provider") {
    const provider = providers.find((entry) => entry.id === view.providerId);
    return provider ? getProviderModelRows(provider).length : 0;
  }
  // Keep the search frame stable while typing instead of resizing per keystroke.
  if (isSearching) {
    return getAllProviderModelRows(providers).length;
  }
  if (profileRowCount === null) {
    return providers.length;
  }
  // Profiles heading + rows (or the lone "Create profile" row), then the "Providers" heading.
  const profileSectionRows = profileRowCount === 0 ? 1 : profileRowCount + 1;
  return profileSectionRows + 1 + providers.length;
}
