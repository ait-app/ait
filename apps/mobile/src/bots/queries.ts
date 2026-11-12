import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { BotTurn, BotView } from "@ait/protocol/bot/types";
import { useHostRuntimeClient } from "@/runtime/host-runtime";

/**
 * Query keys for the Workspace Bot. Conversation and channel keys carry the Bot's builtin
 * channel id, so a Bot replaced on another device never reuses what its predecessor cached.
 */
export const botKeys = {
  bot: (serverId: string, workspaceId: string) => ["bot", serverId, workspaceId],
  conversation: (serverId: string, workspaceId: string, builtinChannelId: string) => [
    "bot-conversation",
    serverId,
    workspaceId,
    builtinChannelId,
  ],
  history: (serverId: string, workspaceId: string, conversationId?: string) => [
    "bot-history",
    serverId,
    workspaceId,
    conversationId,
  ],
  memory: (serverId: string, workspaceId: string) => ["bot-memory", serverId, workspaceId],
  notifications: (serverId: string, workspaceId: string, builtinChannelId: string) => [
    "bot-notifications",
    serverId,
    workspaceId,
    builtinChannelId,
  ],
  /** Every ticket list of a Bot, open-only or with closed ones. */
  tickets: (serverId: string, workspaceId: string, builtinChannelId: string) => [
    "bot-tickets",
    serverId,
    workspaceId,
    builtinChannelId,
  ],
  ticket: (serverId: string, workspaceId: string, ticketId: string) => [
    "bot-ticket",
    serverId,
    workspaceId,
    ticketId,
  ],
  channels: (serverId: string, workspaceId: string, builtinChannelId: string) => [
    "bot-channels",
    serverId,
    workspaceId,
    builtinChannelId,
  ],
};

/** Whether a Bot request failed with the host error `code`, such as `bot_conflict`. */
export function hasBotErrorCode(cause: unknown, code: string): boolean {
  return cause instanceof Error && "code" in cause && cause.code === code;
}

/** Whether a Bot request failed with the host's `BotConflict` error. */
export function isBotConflict(cause: unknown): boolean {
  return hasBotErrorCode(cause, "bot_conflict");
}

export function isBotTurnPending(turn: BotTurn): boolean {
  return ["queued", "sending", "running", "needs_approval"].includes(turn.status);
}

export function useWorkspaceBot(serverId: string, workspaceId: string, enabled: boolean) {
  const client = useHostRuntimeClient(serverId);
  return useQuery({
    queryKey: botKeys.bot(serverId, workspaceId),
    enabled: Boolean(enabled && client),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data === null ? 5_000 : 10_000),
    queryFn: async (): Promise<BotView | null> => {
      if (!client) throw new Error("Bot unavailable");
      return (await client.botRequest("bot.get.request", { workspaceId })).bot;
    },
  });
}

/** Refresh pending receipts and read only new inputs after the cached sequence. */
export async function fetchBotHistory(
  client: Pick<DaemonClient, "botRequest">,
  workspaceId: string,
  conversationId: string,
  cached: BotTurn[] = [],
): Promise<BotTurn[]> {
  const pending = await Promise.all(
    cached
      .filter(isBotTurnPending)
      .map(
        async (turn) =>
          (await client.botRequest("bot.turn.get.request", { workspaceId, turnId: turn.id })).turn,
      ),
  );
  const merged = new Map(cached.map((turn) => [turn.id, turn]));
  pending.forEach((turn) => merged.set(turn.id, turn));
  let afterSequence = cached.reduce((last, turn) => Math.max(last, turn.sequence), 0);
  for (;;) {
    const page = await client.botRequest("bot.history.get.request", {
      workspaceId,
      conversationId,
      afterSequence,
      limit: 50,
    });
    page.turns.forEach((turn) => merged.set(turn.id, turn));
    if (page.turns.length < 50 || page.nextSequence === null || page.nextSequence <= afterSequence)
      break;
    afterSequence = page.nextSequence;
  }
  return [...merged.values()].sort((left, right) => left.sequence - right.sequence);
}

export function useBotHistory(
  serverId: string,
  workspaceId: string,
  conversationId: string | undefined,
  enabled: boolean,
) {
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const queryKey = botKeys.history(serverId, workspaceId, conversationId);
  return useQuery({
    queryKey,
    enabled: Boolean(enabled && conversationId && client),
    refetchInterval: (query) => (query.state.data?.some(isBotTurnPending) ? 1000 : 3000),
    queryFn: async () => {
      if (!client || !conversationId) throw new Error("Bot unavailable");
      const cached = queryClient.getQueryData<BotTurn[]>(queryKey);
      const fetched = await fetchBotHistory(client, workspaceId, conversationId, cached);
      // A message may have been accepted while the history read was in flight.
      const latest = queryClient.getQueryData<BotTurn[]>(queryKey) ?? [];
      const merged = new Map(latest.map((turn) => [turn.id, turn]));
      fetched.forEach((turn) => merged.set(turn.id, turn));
      return [...merged.values()].sort((left, right) => left.sequence - right.sequence);
    },
  });
}
