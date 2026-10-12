import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { BotTurn } from "@ait/protocol/bot/types";
import { useHostRuntimeClient } from "@/runtime/host-runtime";

export const botKeys = {
  list: (serverId: string, workspaceId: string) => ["bots", serverId, workspaceId],
  detail: (serverId: string, botId?: string) => ["bot", serverId, botId],
  conversation: (serverId: string, botId: string) => ["bot-conversation", serverId, botId],
  history: (serverId: string, botId: string, conversationId?: string) => [
    "bot-history",
    serverId,
    botId,
    conversationId,
  ],
  memory: (serverId: string, botId: string) => ["bot-memory", serverId, botId],
};

export function isBotTurnPending(turn: BotTurn): boolean {
  return ["queued", "sending", "running", "needs_approval"].includes(turn.status);
}

export function useBotDetail(serverId: string, botId?: string, enabled = true) {
  const client = useHostRuntimeClient(serverId);
  return useQuery({
    queryKey: botKeys.detail(serverId, botId),
    enabled: Boolean(enabled && botId && client),
    staleTime: 10_000,
    queryFn: async () => {
      if (!client || !botId) throw new Error("Bot unavailable");
      return (await client.botRequest("bot.get.request", { botId })).bot;
    },
  });
}

/** Refresh pending receipts and read only new inputs after the cached sequence. */
export async function fetchBotHistory(
  client: Pick<DaemonClient, "botRequest">,
  botId: string,
  conversationId: string,
  cached: BotTurn[] = [],
): Promise<BotTurn[]> {
  const pending = await Promise.all(
    cached
      .filter(isBotTurnPending)
      .map(
        async (turn) =>
          (await client.botRequest("bot.turn.get.request", { botId, turnId: turn.id })).turn,
      ),
  );
  const merged = new Map(cached.map((turn) => [turn.id, turn]));
  pending.forEach((turn) => merged.set(turn.id, turn));
  let afterSequence = cached.reduce((last, turn) => Math.max(last, turn.sequence), 0);
  for (;;) {
    const page = await client.botRequest("bot.history.get.request", {
      botId,
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
  botId: string,
  conversationId: string | undefined,
  enabled: boolean,
) {
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const queryKey = botKeys.history(serverId, botId, conversationId);
  return useQuery({
    queryKey,
    enabled: Boolean(enabled && conversationId && client),
    refetchInterval: (query) => (query.state.data?.some(isBotTurnPending) ? 1000 : 3000),
    queryFn: async () => {
      if (!client || !conversationId) throw new Error("Bot unavailable");
      const cached = queryClient.getQueryData<BotTurn[]>(queryKey);
      const fetched = await fetchBotHistory(client, botId, conversationId, cached);
      // A message may have been accepted while the history read was in flight.
      const latest = queryClient.getQueryData<BotTurn[]>(queryKey) ?? [];
      const merged = new Map(latest.map((turn) => [turn.id, turn]));
      fetched.forEach((turn) => merged.set(turn.id, turn));
      return [...merged.values()].sort((left, right) => left.sequence - right.sequence);
    },
  });
}
