import type { HubLinkState } from "./link.js";

/** Public state contains no tickets, resume tokens or node credentials. */
export interface HubAccountSnapshot {
  state: HubLinkState;
  linkId: string | null;
  centerUrl: string | null;
  methods: readonly string[];
}

/** Account event identity is independent of the transport sequence and resource ID. */
export interface HubAccountEvent {
  subscriptionId: string;
  cursor: number;
  hostId: string;
  eventId: string;
  resourceId?: string | null;
  method: string;
  params: unknown;
  /** Desktop's shared link can suppress alerts for a resource focused in another window. */
  notificationSuppressed?: boolean;
}

/** Account services can live in the current process or behind the desktop IPC boundary. */
export interface HubAccountConnection {
  snapshot(): HubAccountSnapshot;
  subscribe(listener: () => void): () => void;
  call<T = unknown>(method: string, params: unknown): Promise<T>;
  onEvent(listener: (event: HubAccountEvent) => void): () => void;
}

/** Only access-side services may be invoked by a renderer. */
export const HUB_ACCESS_METHODS = [
  "push.register",
  "push.unregister.request",
  "session.heartbeat",
  "session.events.set_subscription.request",
  "server.status.subscribe",
  "creation.subscribe.request",
  "agent.timeline.set_subscription.request",
  "subscription.release.request",
  "project.list.request",
  "workspace.list.request",
  "agent.list.request",
  "hub.events.get_cursor",
] as const;

export interface HubDirectoryItem {
  hostId: string;
  id: string;
  data: Record<string, unknown>;
  revision: number;
  updatedAt: string;
  online: boolean;
  stale: boolean;
}

export interface HubDirectoryPage {
  items: HubDirectoryItem[];
  nextCursor: { hostId: string; id: string } | null;
}
