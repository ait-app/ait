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
  "share.subscribe",
  "org.members.list",
  "share.revoke",
] as const;

/** One organization of the account, as pushed in `orgs` snapshots after `share.subscribe`. */
export interface HubOrganization {
  organizationId: string;
  name: string;
  slug: string;
  status: string;
  myRole: string;
  /** Changes whenever membership changes; reload `org.members.list` when it does. */
  memberVersion: number;
}

/** A workspace another account shares with this one, from `share_presence` snapshots. */
export interface HubSharedWorkspace {
  shareId: string;
  organizationId: string;
  organizationName: string;
  hostId: string;
  /** Serve endpoint to open the guest session to, together with `shareId`. */
  nodeId: string;
  hostName: string;
  owner: { userId: string; displayName: string };
  workspaceKey: string;
  displayName: string;
  projectName: string | null;
  permission: string;
  state: "online" | "reconnecting";
}

/** One member of an organization, as returned by `org.members.list`. */
export interface HubOrganizationMember {
  userId: string;
  displayName: string;
  email: string | null;
  phoneNumber: string | null;
  role: string;
  joinedAt: string;
}

/** Result of `org.members.list`. */
export interface HubOrganizationMembers {
  memberVersion: number;
  members: HubOrganizationMember[];
}

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
