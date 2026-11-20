import { useTranslation } from "react-i18next";
import {
  ForgeSearchItemSchema,
  GitHubSearchItemSchema,
  type ForgeAuthState,
  type ForgeSearchItem,
  ForgeSearchKind,
  type ForgeSearchResponse,
  type GitHubSearchResponse,
} from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";
import { useFetchQuery } from "@/data/query";
import { parseForgeAuthState } from "@/git/forge";

const FORGE_SEARCH_STALE_TIME = 30_000;

export interface ForgeSearchPayload {
  items: ForgeSearchItem[];
  authState: ForgeAuthState;
  error: string | null;
  requestId: string;
}

/** "all" also reaches merged and closed items; the default is open items only. */
export type ForgeSearchState = "open" | "all";

interface ForgeSearchOptions {
  cwd: string;
  query: string;
  limit?: number;
  kinds?: ForgeSearchKind[];
  state?: ForgeSearchState;
}

interface LegacyGitHubSearchOptions {
  cwd: string;
  query: string;
  limit?: number;
  kinds?: LegacyGitHubSearchKind[];
}

export interface ForgeSearchClient {
  searchForge: (
    options: ForgeSearchOptions,
    requestId?: string,
  ) => Promise<ForgeSearchResponse["payload"]>;
  searchGitHub?: (
    options: LegacyGitHubSearchOptions,
    requestId?: string,
  ) => Promise<GitHubSearchResponse["payload"]>;
}

type LegacyGitHubSearchKind = "github-issue" | "github-pr";

interface ForgeSearchQueryInput {
  client: ForgeSearchClient | null;
  serverId: string;
  cwd: string;
  query: string;
  kinds?: ForgeSearchKind[];
  /**
   * Which item states to search. A daemon that predates the option, and the legacy GitHub
   * transport, search open items whatever is asked for.
   */
  state?: ForgeSearchState;
  enabled: boolean;
  supportsForgeSearch?: boolean;
  hostDisconnectedMessage?: string;
}

export function forgeSearchQueryKey(
  serverId: string,
  cwd: string,
  query: string,
  kinds?: ForgeSearchKind[],
  transport: "forge" | "github" = "forge",
  state: ForgeSearchState = "open",
) {
  const trimmedQuery = query.trim();
  const key: string[] = ["forge-search", serverId, cwd, transport, trimmedQuery];
  if (kinds) key.push([...kinds].sort().join(","));
  // Open-only searches keep the key they always had; a wider search must not share their results.
  if (state === "all") key.push("all-states");
  return key;
}

export function buildForgeSearchQueryOptions(input: ForgeSearchQueryInput) {
  const query = input.query.trim();
  const transport = input.supportsForgeSearch === true ? "forge" : "github";

  return {
    queryKey: forgeSearchQueryKey(
      input.serverId,
      input.cwd,
      query,
      input.kinds,
      transport,
      input.state,
    ),
    queryFn: async (): Promise<ForgeSearchPayload> => {
      if (!input.client) {
        throw new Error(
          input.hostDisconnectedMessage ?? i18n.t("workspace.terminal.hostDisconnected"),
        );
      }
      const request: ForgeSearchOptions = { cwd: input.cwd, query, limit: 20 };
      if (input.kinds) request.kinds = input.kinds;
      if (input.state === "all") request.state = "all";
      // COMPAT(githubSearchRpc): use the legacy GitHub RPC with daemons that
      // predate forge.search.*. Remove after 2027-01-17 once the supported
      // daemon floor is >= v0.2.0.
      if (transport === "github" && input.client.searchGitHub) {
        return normalizeLegacyGitHubSearchPayload(
          await input.client.searchGitHub(toLegacyGitHubSearchRequest(request)),
        );
      }
      return normalizeForgeSearchPayload(await input.client.searchForge(request));
    },
    enabled: input.enabled && Boolean(input.client),
    dataShape: "list" as const,
    staleTimeMs: FORGE_SEARCH_STALE_TIME,
  };
}

function normalizeForgeSearchPayload(payload: ForgeSearchResponse["payload"]): ForgeSearchPayload {
  return {
    items: payload.items.flatMap((item) => {
      const result = ForgeSearchItemSchema.safeParse(item);
      return result.success ? [result.data] : [];
    }),
    authState: parseForgeAuthState(payload.authState) ?? "unauthenticated",
    error: payload.error,
    requestId: payload.requestId,
  };
}

function normalizeLegacyGitHubSearchPayload(
  payload: GitHubSearchResponse["payload"],
): ForgeSearchPayload {
  // COMPAT(githubSearchAuthState): normalize the legacy GitHub search response
  // until 2027-01-17, when the supported daemon floor is >= v0.2.0.
  const featuresEnabled = payload.featuresEnabled ?? payload.githubFeaturesEnabled ?? true;
  return {
    items: payload.items.flatMap((item) => {
      const result = GitHubSearchItemSchema.safeParse(item);
      if (!result.success) {
        return [];
      }
      return [
        result.data.kind === "pr"
          ? { ...result.data, kind: "change_request" as const }
          : { ...result.data, kind: "issue" as const },
      ];
    }),
    authState:
      parseForgeAuthState(payload.authState) ??
      (featuresEnabled ? "authenticated" : "unauthenticated"),
    error: payload.error,
    requestId: payload.requestId,
  };
}

function toLegacyGitHubSearchKind(kind: ForgeSearchKind): LegacyGitHubSearchKind {
  return kind === "change_request" ? "github-pr" : "github-issue";
}

function toLegacyGitHubSearchRequest(request: ForgeSearchOptions): LegacyGitHubSearchOptions {
  const legacy: LegacyGitHubSearchOptions = {
    cwd: request.cwd,
    query: request.query,
    limit: request.limit,
  };
  if (request.kinds) legacy.kinds = request.kinds.map(toLegacyGitHubSearchKind);
  return legacy;
}

export function useForgeSearchQuery(input: ForgeSearchQueryInput) {
  const { t } = useTranslation();
  return useFetchQuery(
    buildForgeSearchQueryOptions({
      ...input,
      hostDisconnectedMessage: t("workspace.terminal.hostDisconnected"),
    }),
  );
}
