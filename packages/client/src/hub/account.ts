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
  "group.members.list",
  "share.revoke",
] as const;

/** One group of the account, as pushed in `groups` snapshots after `share.subscribe`. */
export interface HubGroup {
  groupId: string;
  name: string;
  slug: string;
  status: string;
  myRole: string;
  /** Changes whenever membership changes; reload `group.members.list` when it does. */
  memberVersion: number;
}

/** Kind of resource a share grants: one workspace, one project, or the whole node (Host). */
export type HubShareResourceKind = "workspace" | "project" | "node";

/** A resource another account shares with this one, from `share_presence` snapshots. */
export interface HubShare {
  shareId: string;
  groupId: string;
  groupName: string;
  hostId: string;
  /** Serve endpoint to open the guest session to, together with `shareId`. */
  nodeId: string;
  hostName: string;
  owner: { userId: string; displayName: string };
  resourceKind: HubShareResourceKind;
  /** Workspace ID, project ID, or `""` for a node share. */
  resourceKey: string;
  displayName: string;
  projectName: string | null;
  permission: string;
  state: "online" | "reconnecting";
}

/** One member of a group, as returned by `group.members.list`. */
export interface HubGroupMember {
  userId: string;
  displayName: string;
  email: string | null;
  phoneNumber: string | null;
  role: string;
  joinedAt: string;
}

/** Result of `group.members.list`. */
export interface HubGroupMembers {
  memberVersion: number;
  members: HubGroupMember[];
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
