import { describe, expect, it } from "vitest";
import {
  countOpenTickets,
  isTicketClosed,
  orderTickets,
  reportKindKey,
  reportSourceKey,
  ticketStatusKey,
  ticketWaitingKey,
} from "./tickets";

describe("Bot ticket helpers", () => {
  it("puts waiting tickets first and keeps the host's order within each group", () => {
    const tickets = [
      { id: "open-new", status: "open" },
      { id: "closed-new", status: "done" },
      { id: "waiting-new", status: "waiting" },
      { id: "later", status: "status_from_a_newer_host" },
      { id: "closed-old", status: "cancelled" },
      { id: "waiting-old", status: "waiting" },
      { id: "open-old", status: "open" },
    ];
    expect(orderTickets(tickets).map((ticket) => ticket.id)).toEqual([
      "waiting-new",
      "waiting-old",
      "open-new",
      "later",
      "open-old",
      "closed-new",
      "closed-old",
    ]);
    expect(countOpenTickets(tickets)).toEqual({ open: 5, waiting: 2 });
    expect(["done", "failed", "cancelled"].every(isTicketClosed)).toBe(true);
    expect(["open", "waiting", "status_from_a_newer_host"].some(isTicketClosed)).toBe(false);
  });

  it("labels known values and reads values from newer hosts generically", () => {
    expect(ticketStatusKey("waiting")).toBe("bots.tickets.status.waiting");
    expect(ticketStatusKey("paused")).toBe("bots.tickets.status.other");
    expect(
      ticketWaitingKey({ reason: "permission", agentId: "a", since: "2026-11-13T00:00:00Z" }),
    ).toBe("bots.tickets.waiting.permission");
    expect(ticketWaitingKey({ reason: "review", agentId: "a", since: "x" })).toBe(
      "bots.tickets.waiting.other",
    );
    expect(ticketWaitingKey(null)).toBe("bots.tickets.waiting.other");
    expect(reportSourceKey("system")).toBe("bots.tickets.source.system");
    expect(reportSourceKey("webhook")).toBe("bots.tickets.source.other");
    expect(reportKindKey("agent_permission")).toBe("bots.tickets.report.agent_permission");
    expect(reportKindKey("milestone")).toBe("bots.tickets.report.other");
  });
});
