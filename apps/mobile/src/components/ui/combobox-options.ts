import {
  compareMatchScores,
  type MatchScore,
  scoreTextFields,
} from "@ait/protocol/search/text-match";

type ComboboxOptionKind = "directory" | "file";

export interface ComboboxOptionModel {
  id: string;
  label: string;
  description?: string;
  kind?: ComboboxOptionKind;
}

const DESCRIPTION_FALLBACK_TIER = 99;

function scoreOption(opt: ComboboxOptionModel, search: string): MatchScore | null {
  const best = scoreTextFields(search, [opt.label, opt.id]);
  if (best) return best;
  if (!opt.description) return null;
  const descriptionScore = scoreTextFields(search, [opt.description]);
  if (!descriptionScore) return null;
  return { ...descriptionScore, tier: descriptionScore.tier + DESCRIPTION_FALLBACK_TIER };
}

export function filterAndRankComboboxOptions(
  options: ComboboxOptionModel[],
  search: string,
): ComboboxOptionModel[] {
  if (!search) return options;
  const scored: { opt: ComboboxOptionModel; score: MatchScore }[] = [];
  for (const opt of options) {
    const score = scoreOption(opt, search);
    if (score) scored.push({ opt, score });
  }
  scored.sort((a, b) => {
    const cmp = compareMatchScores(a.score, b.score);
    if (cmp !== 0) return cmp;
    return a.opt.label.localeCompare(b.opt.label);
  });
  return scored.map((entry) => entry.opt);
}
