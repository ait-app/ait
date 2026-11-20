import type { ForgeSearchItem } from "@ait/protocol/messages";
import { getForgePresentation, normalizeForge } from "@/git/forge";
import { parseForgeRef, type ForgeRef } from "@/git/forge-refs";
import { CLIENT_FORGE_LOGIC_MODULES } from "@/git/forges";

export type ForgeLinkState = "open" | "merged" | "closed";

export interface ForgeLinkDiffStat {
  additions: number;
  deletions: number;
  changedFiles: number | null;
}

export interface ForgeLinkDetails {
  kind: ForgeSearchItem["kind"];
  title: string;
  /** Repository and number as the forge writes them, e.g. `acme/app #795`. */
  reference: string;
  state: ForgeLinkState;
  author: string | null;
  authorAvatarUrl: string | null;
  updatedAt: Date | null;
  diffStat: ForgeLinkDiffStat | null;
}

const REFERENCE_INFIXES = [
  ...new Set(
    CLIENT_FORGE_LOGIC_MODULES.flatMap(
      (module) => module.urlGrammar?.referencePaths?.map((path) => path.infix) ?? [],
    ),
  ),
];

/**
 * Whether a link is shaped like an issue or change-request URL on any known forge. A cheap test
 * for render time, when the workspace's repository is not consulted: it decides which links are
 * worth giving a card at all, and `resolveForgeLinkRef` decides whether one opens.
 */
export function isForgeReferenceLink(href: string): boolean {
  if (!/^https?:\/\//iu.test(href)) return false;
  return REFERENCE_INFIXES.some((infix) => {
    const start = href.indexOf(infix);
    return start >= 0 && /^\d/u.test(href.slice(start + infix.length));
  });
}

/**
 * The issue or change request a link points at, when it belongs to the workspace's own
 * repository. Nothing else can be looked up: forge search is scoped to that repository.
 */
export function resolveForgeLinkRef(
  href: string,
  remoteUrl: string | null | undefined,
): ForgeRef | null {
  return parseForgeRef(href, remoteUrl);
}

export function selectForgeLinkItem(
  items: readonly ForgeSearchItem[],
  ref: ForgeRef,
): ForgeSearchItem | null {
  return items.find((item) => item.kind === ref.kind && item.number === ref.number) ?? null;
}

export function describeForgeLinkItem(item: ForgeSearchItem): ForgeLinkDetails {
  const presentation = getForgePresentation(normalizeForge(item.forge));
  const prefix = item.kind === "issue" ? presentation.issueNumberPrefix : presentation.numberPrefix;
  const number = `${prefix}${item.number}`;
  return {
    kind: item.kind,
    title: item.title,
    reference: item.projectPath ? `${item.projectPath} ${number}` : number,
    state: forgeLinkState(item.state),
    author: item.author?.trim() || null,
    authorAvatarUrl: item.authorAvatarUrl?.trim() || null,
    updatedAt: parseTimestamp(item.updatedAt),
    diffStat: forgeLinkDiffStat(item),
  };
}

function forgeLinkState(state: string): ForgeLinkState {
  const normalized = state.toLowerCase();
  if (normalized === "merged") return "merged";
  // GitLab reports an open issue as "opened".
  if (normalized === "open" || normalized === "opened") return "open";
  return "closed";
}

function forgeLinkDiffStat(item: ForgeSearchItem): ForgeLinkDiffStat | null {
  if (item.additions === undefined || item.deletions === undefined) return null;
  return {
    additions: item.additions,
    deletions: item.deletions,
    changedFiles: item.changedFiles ?? null,
  };
}

function parseTimestamp(value: string | undefined): Date | null {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}
