import { SessionInboundMessageSchema } from "@ait/protocol/messages";
import { LegacyWorkspaces } from "./legacy-workspaces.js";
import { BrowserAutomationHostCapabilitySchema } from "@ait/protocol/browser-automation/capabilities";
import type {
  ServerInfoStatusPayload,
  SessionInboundMessage,
  SessionOutboundMessage,
} from "@ait/protocol/messages";

export type ObservationRequest = { type: SessionInboundMessage["type"] } & Record<string, unknown>;
interface Interest {
  id: string;
  query: ObservationRequest;
}

// COMPAT(ownedSubscriptions): added in v0.8.0; remove after 2027-03-11 once daemon floor >= v0.8.0.
// Legacy directories retain their connection-wide, last-query-wins semantics.
// IDs below identify local listeners, not independent server subscriptions.
export class LegacySubscriptions {
  private readonly interests = new Map<string, Interest>();

  private readonly workspaces: LegacyWorkspaces | null;
  constructor(
    private readonly info: ServerInfoStatusPayload,
    private readonly send: (message: ObservationRequest) => Promise<void>,
    private readonly browserHost: unknown,
  ) {
    this.workspaces = info.features?.workspaceMultiplicity === true ? null : new LegacyWorkspaces();
  }

  normalize(message: SessionOutboundMessage): SessionOutboundMessage {
    return this.workspaces?.normalize(message) ?? message;
  }

  prepareRequest(message: SessionInboundMessage): {
    message: SessionInboundMessage;
    receive(message: SessionOutboundMessage): SessionOutboundMessage;
    finish(): Promise<void>;
  } {
    const identity = {
      message,
      receive: (value: SessionOutboundMessage) => value,
      finish: async () => {},
    };
    const workspaces = this.workspaces;
    if (workspaces && message.type === "workspace.list.request") {
      return {
        ...identity,
        message: SessionInboundMessageSchema.parse({
          type: "agent.list.request",
          requestId: message.requestId,
          scope: "active",
          sort: [{ key: "updated_at", direction: "desc" }],
          page: message.page,
          subscribe: message.subscribe,
        }),
        receive: (value) => {
          if (value.type !== "agent.list.response") return value;
          if (value.payload.requestId !== message.requestId) return value;
          return {
            type: "workspace.list.response",
            payload: {
              ...value.payload,
              entries: workspaces.read(value.payload.entries, !message.page?.cursor),
              emptyProjects: [],
            },
          };
        },
      };
    }
    if (message.type === "terminal.list.subscribe.request") {
      return {
        ...identity,
        receive: (value) => {
          if (value.type !== "terminal.list.changed" || value.payload.cwd !== message.cwd)
            return value;
          return {
            ...value,
            payload: { ...value.payload, requestId: message.requestId },
          };
        },
      };
    }
    if (message.type === "workspace.label.list.request" && !message.subscribe) {
      return {
        ...identity,
        message: {
          ...message,
          subscribe: { subscriptionId: `legacy:${crypto.randomUUID()}` },
        },
      };
    }
    if (message.type === "checkout.diff.get.request") {
      const subscriptionId = `legacy:${crypto.randomUUID()}`;
      return {
        message: SessionInboundMessageSchema.parse({
          ...message,
          type: "checkout.diff.subscribe.request",
          subscriptionId,
        }),
        receive: (value) => {
          if (value.type !== "checkout.diff.subscribe.response") return value;
          if (value.payload.requestId !== message.requestId) return value;
          return { ...value, type: "checkout.diff.get.response" };
        },
        finish: () => this.send({ type: "checkout.diff.unsubscribe.request", subscriptionId }),
      };
    }
    return identity;
  }

  start(query: ObservationRequest): Interest {
    const interest = { id: `legacy:${crypto.randomUUID()}`, query };
    this.interests.set(interest.id, interest);
    return interest;
  }

  request({ id, query }: Interest): ObservationRequest | null {
    switch (query.type) {
      case "agent.list.request":
      case "workspace.list.request":
      case "workspace.label.list.request":
        return { ...query, subscribe: { subscriptionId: id } };
      case "fs.file.subscribe.request":
      case "checkout.diff.subscribe.request":
        return { ...query, subscriptionId: id };
      case "agent.timeline.set_subscription.request":
        if (!this.info.features?.selectiveAgentTimeline) return null;
        return { ...query, agentIds: this.members(query.type, "agentIds") };
      case "session.events.set_subscription.request":
        if (!this.info.features?.explicitEventSubscriptions) return null;
        return {
          ...query,
          events: this.members(query.type, "events").filter(isLegacyEvent),
        };
      case "browser.host.register.request":
        if (!matchesBrowserRegistration(this.browserHost, query)) {
          throw new Error(
            "This daemon requires browser_host registration in the client's hello capabilities",
          );
        }
        return null; // Older hosts register in hello.
      default:
        return query;
    }
  }

  private members(type: ObservationRequest["type"], field: string): string[] {
    const members = new Set<string>();
    for (const interest of this.interests.values()) {
      if (interest.query.type !== type) continue;
      for (const value of interest.query[field] as string[]) members.add(value);
    }
    return [...members].sort();
  }

  release(id: string): ObservationRequest | null {
    const interest = this.interests.get(id);
    if (!interest) return null;
    this.interests.delete(id);
    const { query } = interest;
    switch (query.type) {
      case "agent.timeline.set_subscription.request":
      case "session.events.set_subscription.request":
        return this.request(interest);
      case "fs.file.subscribe.request":
        return { type: "fs.file.unsubscribe.request", subscriptionId: id };
      case "checkout.diff.subscribe.request":
        return { type: "checkout.diff.unsubscribe.request", subscriptionId: id };
      case "terminal.subscribe.request":
        if (
          [...this.interests.values()].some(
            (item) => item.query.type === query.type && item.query.terminalId === query.terminalId,
          )
        )
          return null;
        return { type: "terminal.unsubscribe.request", terminalId: query.terminalId };
      case "terminal.list.subscribe.request":
        if (
          [...this.interests.values()].some(
            (item) =>
              item.query.type === query.type &&
              item.query.cwd === query.cwd &&
              item.query.workspaceId === query.workspaceId,
          )
        )
          return null;
        return {
          type: "terminal.list.unsubscribe.request",
          cwd: query.cwd,
          workspaceId: query.workspaceId,
        };
      default:
        // Directory slots have no release RPC. Detach the local listener.
        return null;
    }
  }

  owns(message: SessionOutboundMessage): boolean {
    return [...this.interests.values()].some((interest) => this.matches(interest, message));
  }

  receive(
    message: SessionOutboundMessage,
    deliver: (message: SessionOutboundMessage) => void,
  ): void {
    const updates: SessionOutboundMessage[] = [
      message,
      ...(this.workspaces?.update(message) ?? []),
    ];
    if (message.type === "agent.stream" && message.payload.event.type === "attention_required") {
      const { agentId, event } = message.payload;
      updates.push({
        type: "agent.attention.required",
        payload: {
          agentId,
          reason: event.reason,
          timestamp: event.timestamp,
          shouldNotify: event.shouldNotify,
          ...(event.notification ? { notification: event.notification } : {}),
        },
      });
    }
    for (const update of updates)
      for (const interest of this.interests.values()) {
        if (!this.matches(interest, update)) continue;
        // Normalize at the connection edge; all consumers receive the same handle shape.
        deliver(
          ("payload" in update
            ? { ...update, payload: { ...update.payload, subscriptionId: interest.id } }
            : { ...update, subscriptionId: interest.id }) as SessionOutboundMessage,
        );
      }
  }

  private matches({ id, query }: Interest, message: SessionOutboundMessage): boolean {
    switch (query.type) {
      case "agent.list.request":
        return message.type === "agent.update";
      case "workspace.list.request":
        return message.type === "workspace.update";
      case "workspace.label.list.request":
        return message.type === "workspace.label.update";
      case "fs.file.subscribe.request":
        return message.type === "fs.file.update" && message.payload.subscriptionId === id;
      case "checkout.diff.subscribe.request":
        return message.type === "checkout.diff.update" && message.payload.subscriptionId === id;
      case "terminal.list.subscribe.request":
        return message.type === "terminal.list.changed" && message.payload.cwd === query.cwd;
      case "terminal.subscribe.request":
        return (
          message.type === "terminal.stream.exit" && message.payload.terminalId === query.terminalId
        );
      case "agent.timeline.set_subscription.request":
        return (
          (message.type === "agent.stream" || message.type === "agent.timeline.replacement") &&
          (query.agentIds as string[]).includes(message.payload.agentId)
        );
      case "session.events.set_subscription.request":
        return (query.events as string[]).includes(
          message.type === "status" ? `status.${message.payload.status}` : message.type,
        );
      case "browser.host.register.request":
        return message.type === "browser.automation.execute.request";
      default:
        return false;
    }
  }
}

function isLegacyEvent(event: string): boolean {
  return [
    "project.update",
    "provider.snapshot.update",
    "agent.attention.required",
    "agent.permission.request",
    "agent.permission.resolved",
  ].includes(event);
}

function matchesBrowserRegistration(advertised: unknown, request: ObservationRequest): boolean {
  const host = BrowserAutomationHostCapabilitySchema.safeParse(advertised);
  if (!host.success) return false;
  if (host.data.hostKind !== request.hostKind) return false;
  for (const command of request.supportedCommands as string[]) {
    if (!host.data.supportedCommands.some((supported) => supported === command)) return false;
  }
  return true;
}
