import type { CommandOwnership } from "./index";
import {
  AgentCommandSessionCatalogUnavailable,
  commandOwnershipForCatalog,
  type AgentCommandCatalog,
} from "@/hooks/use-agent-commands-query";

export type CommandPreflight =
  | { status: "ready"; ownership: CommandOwnership }
  | { status: "unavailable" | "session-unavailable" | "stale" };

/** Check the current native catalog without allowing late responses to authorize a shortcut. */
export async function checkCommandPreflight(input: {
  supportsOwnership: boolean | undefined;
  fetchCatalog: () => Promise<AgentCommandCatalog>;
  isCurrent: () => boolean;
}): Promise<CommandPreflight> {
  if (input.supportsOwnership === undefined) return { status: "unavailable" };
  try {
    const catalog = input.supportsOwnership ? await input.fetchCatalog() : undefined;
    if (!input.isCurrent()) return { status: "stale" };
    const ownership = commandOwnershipForCatalog(input.supportsOwnership, catalog);
    return ownership.kind === "unavailable"
      ? { status: "unavailable" }
      : { status: "ready", ownership };
  } catch (error) {
    if (!input.isCurrent()) return { status: "stale" };
    return {
      status:
        error instanceof AgentCommandSessionCatalogUnavailable
          ? "session-unavailable"
          : "unavailable",
    };
  }
}
