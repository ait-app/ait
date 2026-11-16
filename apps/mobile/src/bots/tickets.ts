import {
  BOT_TICKET_REPORT_KINDS,
  BOT_TICKET_REPORT_SOURCES,
  BOT_TICKET_STATUSES,
  BOT_TICKET_WAIT_REASONS,
  type BotTicketWaiting,
} from "@ait/protocol/bot/types";

const CLOSED_STATUSES: ReadonlySet<string> = new Set(["done", "failed", "cancelled"]);

/** Whether only the Bot or the owner can bring the ticket back; a later host's status is not. */
export function isTicketClosed(status: string): boolean {
  return CLOSED_STATUSES.has(status);
}

function rank(status: string): number {
  if (status === "waiting") return 0;
  return isTicketClosed(status) ? 2 : 1;
}

/**
 * Tickets waiting on the user first, then the other open ones, then closed ones. Within each group
 * the host's order stays: most recently changed open tickets, most recently closed ones.
 */
export function orderTickets<T extends { status: string }>(tickets: readonly T[]): T[] {
  return tickets
    .map((ticket, index) => ({ ticket, index }))
    .sort(
      (left, right) =>
        rank(left.ticket.status) - rank(right.ticket.status) || left.index - right.index,
    )
    .map((entry) => entry.ticket);
}

/** How many tickets are open, and how many of those wait on the user. */
export function countOpenTickets(tickets: readonly { status: string }[]): {
  open: number;
  waiting: number;
} {
  const open = tickets.filter((ticket) => !isTicketClosed(ticket.status));
  return {
    open: open.length,
    waiting: open.filter((ticket) => ticket.status === "waiting").length,
  };
}

function known(values: readonly string[], value: string): string {
  return values.includes(value) ? value : "other";
}

/** Translation key of a ticket status; a later host's status reads generically. */
export function ticketStatusKey(status: string): string {
  return `bots.tickets.status.${known(BOT_TICKET_STATUSES, status)}`;
}

/** Translation key of why a ticket waits, in plain words. */
export function ticketWaitingKey(waiting: BotTicketWaiting | null | undefined): string {
  return `bots.tickets.waiting.${known(BOT_TICKET_WAIT_REASONS, waiting?.reason ?? "")}`;
}

/** Translation key of who wrote a report. */
export function reportSourceKey(source: string): string {
  return `bots.tickets.source.${known(BOT_TICKET_REPORT_SOURCES, source)}`;
}

/** Translation key of what a report says. */
export function reportKindKey(kind: string): string {
  return `bots.tickets.report.${known(BOT_TICKET_REPORT_KINDS, kind)}`;
}
