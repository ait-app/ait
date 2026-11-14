import React, { useMemo, useState } from "react";
import { Text, View } from "react-native";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, CircleDot, CircleSlash, CircleX } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { BotTicketSummary, BotView } from "@ait/protocol/bot/types";
import {
  ComposerTrackPill,
  ComposerTrackRow,
  type ComposerTrackPillSegment,
} from "@/composer/tracks";
import { useHostFeature } from "@/runtime/host-features";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import type { Theme } from "@/styles/theme";
import { botKeys } from "./queries";
import { botTicketStyles as ticketStyles } from "./styles";
import {
  countOpenTickets,
  isTicketClosed,
  orderTickets,
  reportKindKey,
  reportSourceKey,
  ticketStatusKey,
  ticketWaitingKey,
} from "./tickets";

const ThemedCircleAlert = withUnistyles(CircleAlert);
const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircleDot = withUnistyles(CircleDot);
const ThemedCircleSlash = withUnistyles(CircleSlash);
const ThemedCircleX = withUnistyles(CircleX);
const mutedColor = (theme: Theme) => ({ color: theme.colors.foregroundMuted });
const successColor = (theme: Theme) => ({ color: theme.colors.statusSuccess });
const dangerColor = (theme: Theme) => ({ color: theme.colors.statusDanger });
const warningColor = (theme: Theme) => ({ color: theme.colors.statusWarning });

/** Open tickets fit in one default page: the host keeps at most 32 open per Bot. */
const OPEN_LIMIT = 50;
/** Recently closed tickets shown on request; the host keeps up to 64. */
const WITH_CLOSED_LIMIT = 64;

/**
 * Whether the host implements Bot tickets (`bot.ticket.*`). Without it the client shows no tasks
 * and sends no ticket request.
 */
export function useBotTicketsSupported(serverId: string): boolean {
  return useHostFeature(serverId, "workspaceBotTickets");
}

/** The Bot's tickets, refreshed as often as the Bot itself while the tab is shown. */
export function useBotTickets(
  serverId: string,
  workspaceId: string,
  bot: BotView,
  enabled: boolean,
  includeClosed: boolean,
) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  return useQuery({
    queryKey: [...botKeys.tickets(serverId, workspaceId, bot.builtinChannelId), includeClosed],
    enabled: enabled && Boolean(client),
    staleTime: 5_000,
    refetchInterval: 10_000,
    // Asking for closed tickets keeps the open ones on screen while the longer list loads.
    placeholderData: keepPreviousData,
    queryFn: async () => {
      if (!client) throw new Error(t("bots.disconnected"));
      return (
        await client.botRequest("bot.ticket.list.request", {
          workspaceId,
          ...(includeClosed ? { includeClosed, limit: WITH_CLOSED_LIMIT } : { limit: OPEN_LIMIT }),
        })
      ).tickets;
    },
  });
}

export function TicketStatusIcon({ status }: { status: string }) {
  switch (status) {
    case "waiting":
      return <ThemedCircleAlert size={14} uniProps={warningColor} />;
    case "done":
      return <ThemedCircleCheck size={14} uniProps={successColor} />;
    case "failed":
      return <ThemedCircleX size={14} uniProps={dangerColor} />;
    case "cancelled":
      return <ThemedCircleSlash size={14} uniProps={mutedColor} />;
    default:
      return <ThemedCircleDot size={14} uniProps={mutedColor} />;
  }
}

/** What a list row says under the title: why it waits, or its status and Agents. */
function ticketFacts(ticket: BotTicketSummary, t: TFunction) {
  const state =
    ticket.status === "waiting"
      ? t(ticketWaitingKey(ticket.waiting))
      : t(ticketStatusKey(ticket.status));
  return `${state} · ${t("bots.tickets.agentCount", { count: ticket.assignments.length })}`;
}

function TicketRow({
  ticket,
  onOpen,
}: {
  ticket: BotTicketSummary;
  onOpen: (ticketId: string) => void;
}) {
  const { t } = useTranslation();
  const latest = ticket.latestReport;
  const latestText = latest
    ? `${t(reportSourceKey(latest.source))}: ${latest.text.trim() || t(reportKindKey(latest.kind))}`
    : null;
  return (
    <ComposerTrackRow
      onPress={() => onOpen(ticket.id)}
      accessibilityLabel={`${ticket.title}, ${t(ticketStatusKey(ticket.status))}`}
      testID={`bot-ticket-row-${ticket.id}`}
    >
      <View accessibilityLabel={t(ticketStatusKey(ticket.status))} accessible>
        <TicketStatusIcon status={ticket.status} />
      </View>
      <View style={ticketStyles.rowText}>
        <Text style={ticketStyles.rowTitle} numberOfLines={1}>
          {ticket.title}
        </Text>
        <Text
          style={ticket.status === "waiting" ? ticketStyles.rowWaiting : ticketStyles.rowFacts}
          numberOfLines={1}
          testID={`bot-ticket-facts-${ticket.id}`}
        >
          {ticketFacts(ticket, t)}
        </Text>
        {latestText ? (
          <Text
            style={ticketStyles.rowFacts}
            numberOfLines={1}
            testID={`bot-ticket-latest-${ticket.id}`}
          >
            {latestText}
          </Text>
        ) : null}
      </View>
    </ComposerTrackRow>
  );
}

/** A line in the panel that is not a ticket: empty states, errors and the closed heading. */
function PanelNote({ text, testID, error }: { text: string; testID?: string; error?: boolean }) {
  return (
    <ComposerTrackRow>
      <Text
        style={error ? ticketStyles.rowWaiting : ticketStyles.rowFacts}
        accessibilityRole={error ? "alert" : undefined}
        testID={testID}
      >
        {text}
      </Text>
    </ComposerTrackRow>
  );
}

/**
 * The Bot's open tasks beside its chat: a pill counting them and those waiting on the user, and a
 * panel listing them, waiting ones first. Closed tasks load only when asked for.
 */
export function BotTicketsTrack({
  serverId,
  workspaceId,
  bot,
  interactive,
  onOpenTicket,
}: {
  serverId: string;
  workspaceId: string;
  bot: BotView;
  interactive: boolean;
  onOpenTicket: (ticketId: string) => void;
}) {
  const { t } = useTranslation();
  const [showClosed, setShowClosed] = useState(false);
  const tickets = useBotTickets(serverId, workspaceId, bot, interactive, showClosed);
  const ordered = useMemo(() => orderTickets(tickets.data ?? []), [tickets.data]);
  const open = ordered.filter((ticket) => !isTicketClosed(ticket.status));
  const closed = ordered.filter((ticket) => isTicketClosed(ticket.status));
  const counts = countOpenTickets(ordered);
  const segments: ComposerTrackPillSegment[] =
    counts.open === 0
      ? [{ bucket: null, text: t("bots.tickets.title") }]
      : [
          ...(counts.waiting > 0
            ? [
                {
                  bucket: "needs_input" as const,
                  text: t("bots.tickets.pillWaiting", { count: counts.waiting }),
                },
              ]
            : []),
          { bucket: null, text: t("bots.tickets.pillOpen", { count: counts.open }) },
        ];
  return (
    <ComposerTrackPill
      segments={segments}
      panelTitle={t("bots.tickets.title")}
      accessibilityLabel={t("bots.tickets.pillLabel", counts)}
      testID="bot-tickets"
    >
      {tickets.error ? <PanelNote text={tickets.error.message} error /> : null}
      {tickets.data && open.length === 0 ? (
        <PanelNote text={t("bots.tickets.empty")} testID="bot-tickets-empty" />
      ) : null}
      {open.map((ticket) => (
        <TicketRow key={ticket.id} ticket={ticket} onOpen={onOpenTicket} />
      ))}
      {showClosed ? (
        <>
          <PanelNote text={t("bots.tickets.closedHeading")} />
          {tickets.isPlaceholderData ? null : closed.length === 0 ? (
            <PanelNote text={t("bots.tickets.noClosed")} testID="bot-tickets-no-closed" />
          ) : (
            closed.map((ticket) => (
              <TicketRow key={ticket.id} ticket={ticket} onOpen={onOpenTicket} />
            ))
          )}
        </>
      ) : null}
      <ComposerTrackRow
        onPress={() => setShowClosed((shown) => !shown)}
        closeOnSelect={false}
        accessibilityLabel={t(showClosed ? "bots.tickets.hideClosed" : "bots.tickets.showClosed")}
        testID="bot-tickets-toggle-closed"
      >
        <Text style={ticketStyles.rowFacts}>
          {t(showClosed ? "bots.tickets.hideClosed" : "bots.tickets.showClosed")}
        </Text>
      </ComposerTrackRow>
    </ComposerTrackPill>
  );
}
