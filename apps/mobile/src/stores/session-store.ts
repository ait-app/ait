import equal from "fast-deep-equal";
import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { ViewedTimelineOwner } from "@/timeline/viewed-timeline-sync";
import {
  appendSubmittedUserMessage,
  handoffCreatedAgentUserMessageToStream,
  removeSubmittedUserMessage,
  type StreamItem,
  type TodoEntry,
  type TimelinePosition,
  type UserMessageItem,
} from "@/types/stream";
import {
  acceptMessageSubmission,
  beginMessageSubmission,
  hasActiveMessageSubmission,
  observeMessageSubmissionCanonical,
  rejectMessageSubmission,
  type MessageSubmissionRecord,
  type MessageSubmissionRejectionOutcome,
} from "@/stores/message-submission-model";
import type { PendingPermission } from "@/types/shared";
import type { ComposerAttachment } from "@/attachments/types";
import type { AgentLifecycleStatus } from "@ait/protocol/agent-lifecycle";
import type {
  AgentPermissionRequest,
  AgentFeature,
  AgentProvider,
  AgentMode,
  AgentCapabilityFlags,
  AgentUsage,
  AgentPersistenceHandle,
} from "@ait/protocol/agent-types";
import type {
  ProviderUiState,
  ServerInfoStatusPayload,
  ProjectPlacementPayload,
  ServerCapabilities,
  WorkspaceDescriptorPayload,
  WorkspaceProjectDescriptorPayload,
} from "@ait/protocol/messages";
import {
  normalizeWorkspaceOpaqueId,
  normalizeWorkspacePath,
  resolveWorkspaceMapKeyByIdentity,
} from "@/utils/workspace-identity";
import { createAgentLastActivityCoalescer } from "@/runtime/activity/last-activity-coalescer";
import type { AgentLastActivityCommitter } from "@/runtime/activity/types";
import {
  buildWorkspaceAgentActivityIndex,
  type WorkspaceAgentActivity,
} from "@/utils/workspace-agent-activity";
import {
  resolveTurnPresentation,
  TURN_LIVENESS_IDLE,
  type TurnLiveness,
  type TurnPresentation,
} from "@/timeline/turn-liveness";

export interface AgentRuntimeInfo {
  provider: AgentProvider;
  sessionId: string | null;
  model?: string | null;
  modeId?: string | null;
  thinkingOptionId?: string | null;
  extra?: Record<string, unknown>;
}

export interface Agent {
  management?: "managed" | "observed";
  observation?: import("@ait/protocol/messages").AgentObservationPayload;
  serverId: string;
  id: string;
  provider: AgentProvider;
  status: AgentLifecycleStatus;
  turn: TurnLiveness;
  createdAt: Date;
  updatedAt: Date;
  lastUserMessageAt: Date | null;
  lastActivityAt: Date;
  capabilities: AgentCapabilityFlags;
  currentModeId: string | null;
  availableModes: AgentMode[];
  pendingPermissions: AgentPermissionRequest[];
  persistence: AgentPersistenceHandle | null;
  runtimeInfo?: AgentRuntimeInfo;
  /** Provider capabilities project side conversations independently of the main turn. */
  providerUi?: ProviderUiState | null;
  lastUsage?: AgentUsage;
  lastError?: string | null;
  title: string | null;
  cwd: string;
  workspaceId?: string;
  model: string | null;
  features?: AgentFeature[];
  thinkingOptionId?: string | null;
  requiresAttention?: boolean;
  attentionReason?: "finished" | "error" | "permission" | null;
  attentionTimestamp?: Date | null;
  archivedAt?: Date | null;
  parentAgentId: string | null;
  labels: Record<string, string>;
  projectPlacement?: ProjectPlacementPayload | null;
}

export interface WorkspaceDescriptor {
  id: string;
  projectId: string;
  projectDisplayName: string;
  projectCustomName?: string | null;
  projectCustomIconRevision?: string | null;
  projectRootPath: string;
  workspaceDirectory: string;
  worktreeSlug?: WorkspaceDescriptorPayload["worktreeSlug"];
  initialBranch?: WorkspaceDescriptorPayload["initialBranch"];
  projectKind: WorkspaceDescriptorPayload["projectKind"];
  workspaceKind: WorkspaceDescriptorPayload["workspaceKind"];
  name: string;
  title?: string | null;
  pinnedAt?: string | null;
  labels?: string[];
  status: WorkspaceDescriptorPayload["status"];
  statusEnteredAt: Date | null;
  archivingAt: string | null;
  diffStat: { additions: number; deletions: number } | null;
  scripts: WorkspaceDescriptorPayload["scripts"];
  gitRuntime?: WorkspaceDescriptorPayload["gitRuntime"];
  githubRuntime?: WorkspaceDescriptorPayload["githubRuntime"];
  forge?: WorkspaceDescriptorPayload["forge"];
  project?: ProjectPlacementPayload;
}

export function normalizeWorkspaceDescriptor(
  payload: WorkspaceDescriptorPayload,
): WorkspaceDescriptor {
  const statusEnteredAtRaw = payload.statusEnteredAt;
  const statusEnteredAt: Date | null =
    typeof statusEnteredAtRaw === "string" && statusEnteredAtRaw.length > 0
      ? new Date(statusEnteredAtRaw)
      : null;
  return {
    id: normalizeWorkspaceOpaqueId(payload.id) ?? payload.id,
    projectId: payload.projectId,
    projectDisplayName: payload.projectDisplayName,
    projectCustomName: payload.projectCustomName ?? null,
    projectCustomIconRevision: payload.projectCustomIconRevision ?? null,
    projectRootPath: payload.projectRootPath,
    // Canonicalize the workspace directory once, at the store boundary, so every
    // consumer can read workspace.workspaceDirectory directly. Empty means "no
    // usable directory" (older daemons may omit it; the wire field is optional).
    workspaceDirectory: normalizeWorkspacePath(payload.workspaceDirectory) ?? "",
    worktreeSlug: payload.worktreeSlug,
    initialBranch: payload.initialBranch,
    projectKind: payload.projectKind,
    workspaceKind: payload.workspaceKind,
    name: payload.name,
    title: payload.title ?? null,
    pinnedAt: payload.pinnedAt ?? null,
    // COMPAT(workspaceLabels): old daemons omit assignments.
    labels: payload.labels ?? [],
    status: payload.status,
    statusEnteredAt,
    archivingAt: payload.archivingAt ?? null,
    diffStat: payload.diffStat ?? null,
    scripts: (payload.scripts ?? []).map((s) => Object.assign({}, s)),
    gitRuntime: payload.gitRuntime,
    githubRuntime: payload.githubRuntime,
    forge: payload.forge,
    project: payload.project,
  };
}

export interface ProjectDescriptor {
  projectId: string;
  projectKey?: string | null;
  projectDisplayName: string;
  projectCustomName: string | null;
  projectCustomIconRevision?: string | null;
  projectIconRevision?: string;
  projectRootPath: string;
  projectKind: WorkspaceDescriptorPayload["projectKind"];
}

export function normalizeProjectDescriptor(
  payload: WorkspaceProjectDescriptorPayload,
): ProjectDescriptor {
  return {
    projectId: payload.projectId,
    projectKey: payload.projectKey ?? null,
    projectDisplayName: payload.projectDisplayName,
    projectCustomName: payload.projectCustomName ?? null,
    projectCustomIconRevision: payload.projectCustomIconRevision ?? null,
    projectIconRevision: payload.projectIconRevision,
    projectRootPath: payload.projectRootPath,
    projectKind: payload.projectKind,
  };
}

function preserveWorkspaceDescriptorIdentity(
  incoming: WorkspaceDescriptor,
  existing?: WorkspaceDescriptor | null,
): WorkspaceDescriptor {
  if (existing && equal(existing, incoming)) {
    return existing;
  }
  return incoming;
}

function preserveMapIdentity<Key, Value>(
  existing: Map<Key, Value>,
  incoming: Map<Key, Value>,
): Map<Key, Value> {
  if (existing === incoming) {
    return existing;
  }

  const next = new Map<Key, Value>();
  let changed = existing.size !== incoming.size;
  const existingEntries = existing.entries();

  for (const [key, workspace] of incoming) {
    const existingWorkspace = existing.get(key);
    const nextWorkspace =
      existingWorkspace && equal(existingWorkspace, workspace) ? existingWorkspace : workspace;
    next.set(key, nextWorkspace);
    const existingEntry = existingEntries.next().value;
    if (!existingEntry || existingEntry[0] !== key || existingEntry[1] !== nextWorkspace) {
      changed = true;
    }
  }

  return changed ? next : existing;
}

export type ExplorerEntryKind = "file" | "directory";
export type ExplorerFileKind = "text" | "image" | "binary";
export type ExplorerEncoding = "utf-8" | "base64" | "none";

export interface ExplorerEntry {
  name: string;
  path: string;
  kind: ExplorerEntryKind;
  size: number;
  modifiedAt: string;
}

export interface ExplorerFile {
  path: string;
  kind: ExplorerFileKind;
  encoding: ExplorerEncoding;
  content?: string;
  // TextDecoder removes a leading UTF-8 BOM; retain this bit so file writes can restore it.
  hasBom: boolean;
  mimeType?: string;
  size: number;
  modifiedAt: string;
  revision?: string;
}

export interface ExplorerDirectory {
  path: string;
  entries: ExplorerEntry[];
}

interface ExplorerRequestState {
  path: string;
  mode: "list" | "file";
}

export interface AgentFileExplorerState {
  directories: Map<string, ExplorerDirectory>;
  files: Map<string, ExplorerFile>;
  isLoading: boolean;
  lastError: string | null;
  pendingRequest: ExplorerRequestState | null;
  currentPath: string;
  history: string[];
  lastVisitedPath: string;
  selectedEntryPath: string | null;
}

export interface DaemonServerInfo {
  serverId: string;
  hostname: string | null;
  version: string | null;
  desktopManaged?: boolean;
  capabilities?: ServerCapabilities;
  features?: ServerInfoStatusPayload["features"];
}

export interface AgentTimelineCursorState {
  epoch: string;
  startSeq: number;
  endSeq: number;
  retainedRanges?: Array<{
    startSeq: number;
    endSeq: number;
    hasOlder?: boolean;
  }>;
}

export type AgentTimelineState =
  | { status: "cold" }
  | { status: "painted"; items: StreamItem[] }
  | {
      status: "synced";
      items: StreamItem[];
      range: AgentTimelineCursorState | null;
      older: "available" | "none";
      newer: "available" | "none";
    };

export function selectAgentTimelineState(
  session: SessionState | undefined,
  agentId: string,
): AgentTimelineState {
  if (!session) return { status: "cold" };
  const items = session.agentStreamTail.get(agentId) ?? [];
  if (session.agentAuthoritativeHistoryApplied.get(agentId) === true) {
    return {
      status: "synced",
      items,
      range: session.agentTimelineCursor.get(agentId) ?? null,
      older: session.agentTimelineHasOlder.get(agentId) === true ? "available" : "none",
      newer: session.agentTimelineHasNewer.get(agentId) === true ? "available" : "none",
    };
  }
  return items.length > 0 ? { status: "painted", items } : { status: "cold" };
}

export function selectAgentTurnPresentation(
  session: SessionState | undefined,
  agentId: string,
): TurnPresentation {
  return resolveTurnPresentation(
    session?.agents.get(agentId)?.turn ??
      session?.agentDetails.get(agentId)?.turn ??
      TURN_LIVENESS_IDLE,
    hasActiveMessageSubmission(session?.messageSubmissions.get(agentId)),
  );
}

function updateAgentTasks(
  current: Map<string, TodoEntry[]>,
  agentId: string,
  taskSnapshot: TodoEntry[] | undefined,
): Map<string, TodoEntry[]> {
  if (taskSnapshot === undefined || equal(current.get(agentId) ?? [], taskSnapshot)) return current;
  const next = new Map(current);
  if (taskSnapshot.length > 0) next.set(agentId, taskSnapshot);
  else next.delete(agentId);
  return next;
}

function updateAgentTaskSnapshot(
  session: SessionState,
  agentId: string,
  taskSnapshot: TodoEntry[] | undefined,
  cursor: TimelinePosition | undefined,
): Pick<SessionState, "agentTasks" | "agentTaskCursor"> {
  const currentCursor = session.agentTaskCursor.get(agentId);
  if (
    taskSnapshot === undefined ||
    (cursor && currentCursor?.epoch === cursor.epoch && currentCursor.seq > cursor.seq)
  ) {
    return { agentTasks: session.agentTasks, agentTaskCursor: session.agentTaskCursor };
  }
  const agentTasks = updateAgentTasks(session.agentTasks, agentId, taskSnapshot);
  const agentTaskCursor =
    cursor && !equal(cursor, currentCursor)
      ? new Map(session.agentTaskCursor).set(agentId, cursor)
      : session.agentTaskCursor;
  return { agentTasks, agentTaskCursor };
}

// Per-session state
export interface SessionState {
  serverId: string;

  // Daemon client (immutable reference)
  client: DaemonClient | null;
  clientGeneration: number;
  viewedTimelineSync: ViewedTimelineOwner | null;

  // Server metadata (from server_info handshake)
  serverInfo: DaemonServerInfo | null;

  // Hydration status
  hasHydratedAgents: boolean;
  hasHydratedWorkspaces: boolean;
  hasWorkspaceDirectorySnapshot: boolean;

  // Focus
  focusedAgentId: string | null;
  focusedTerminalId: string | null;

  // Stream state (head/tail model)
  agentStreamTail: Map<string, StreamItem[]>;
  agentStreamHead: Map<string, StreamItem[]>;
  agentTasks: Map<string, TodoEntry[]>;
  // Current checklist coverage is independent of the transcript window being browsed.
  agentTaskCursor: Map<string, TimelinePosition>;
  messageSubmissions: Map<string, MessageSubmissionRecord[]>;
  agentTimelineCursor: Map<string, AgentTimelineCursorState>;
  agentTimelineHasOlder: Map<string, boolean>;
  agentTimelineHasNewer: Map<string, boolean>;
  agentTimelineOlderFetchInFlight: Map<string, boolean>;
  historySyncGeneration: number;
  agentHistorySyncGeneration: Map<string, number>;
  agentAuthoritativeHistoryApplied: Map<string, boolean>;

  // Initializing agents (used for UI loading state)
  initializingAgents: Map<string, boolean>;

  // Agents
  agents: Map<string, Agent>;
  workspaceAgentActivity: Map<string, WorkspaceAgentActivity>;
  agentDetails: Map<string, Agent>;
  workspaces: Map<string, WorkspaceDescriptor>;
  // All active project descriptors, keyed by host-local projectId.
  projects: Map<string, ProjectDescriptor>;

  // Permissions
  pendingPermissions: Map<string, PendingPermission>;

  // File explorer
  fileExplorer: Map<string, AgentFileExplorerState>;

  // Queued messages
  queuedMessages: Map<
    string,
    Array<{ id: string; text: string; attachments: ComposerAttachment[] }>
  >;
}

// Global store state
interface SessionStoreState {
  sessions: Record<string, SessionState>;

  // Agent activity timestamps (top-level, keyed by agentId to prevent cascade rerenders)
  agentLastActivity: Map<string, Date>;
}

// Action types
interface SessionStoreActions {
  // Session management
  initializeSession: (
    serverId: string,
    client: DaemonClient | null,
    clientGeneration?: number,
  ) => void;
  clearSession: (serverId: string) => void;
  getSession: (serverId: string) => SessionState | undefined;
  updateSessionClient: (serverId: string, client: DaemonClient, clientGeneration?: number) => void;
  setViewedTimelineSync: (serverId: string, sync: ViewedTimelineOwner | null) => void;
  updateSessionServerInfo: (serverId: string, info: DaemonServerInfo) => void;

  // Focus
  setFocusedAgentId: (serverId: string, agentId: string | null) => void;
  setFocusedTerminalId: (serverId: string, terminalId: string | null) => void;

  // Stream state (head/tail model)
  setAgentStreamTail: (serverId: string, state: SessionFieldUpdate<"agentStreamTail">) => void;
  setAgentStreamState: (
    serverId: string,
    agentId: string,
    state: {
      tail?: StreamItem[];
      head?: StreamItem[];
      acknowledgedClientMessageIds?: readonly string[];
      taskSnapshot?: TodoEntry[];
      taskSnapshotCursor?: TimelinePosition;
    },
  ) => void;
  beginAgentMessageSubmission: (
    serverId: string,
    agentId: string,
    message: UserMessageItem,
  ) => void;
  acceptAgentMessageSubmission: (
    serverId: string,
    agentId: string,
    clientMessageId: string,
  ) => void;
  rejectAgentMessageSubmission: (
    serverId: string,
    agentId: string,
    clientMessageId: string,
  ) => MessageSubmissionRejectionOutcome;
  handoffCreatedAgentUserMessage: (
    serverId: string,
    agentId: string,
    message: UserMessageItem,
  ) => boolean;
  clearAgentStreamHead: (serverId: string, agentId: string) => void;
  setAgentTimelineCursor: (
    serverId: string,
    state: SessionFieldUpdate<"agentTimelineCursor">,
  ) => void;
  setAgentTimelineHasNewer: (
    serverId: string,
    state: SessionFieldUpdate<"agentTimelineHasNewer">,
  ) => void;
  setAgentTimelineOlderFetchInFlight: (
    serverId: string,
    state: SessionFieldUpdate<"agentTimelineOlderFetchInFlight">,
  ) => void;
  bumpHistorySyncGeneration: (serverId: string) => void;
  markAgentHistorySynchronized: (serverId: string, agentId: string) => void;
  setAgentAuthoritativeHistoryApplied: (
    serverId: string,
    agentId: string,
    applied: boolean,
  ) => void;
  applyAgentTimelineResponseState: (
    serverId: string,
    agentId: string,
    state: {
      items: StreamItem[];
      head: StreamItem[];
      range: AgentTimelineCursorState | null;
      older: "available" | "none" | "unchanged";
      newer: boolean;
      synchronized: boolean;
      acknowledgedClientMessageIds: string[];
      taskSnapshot?: TodoEntry[];
      taskSnapshotCursor?: TimelinePosition;
    },
  ) => void;

  // Initializing agents
  setInitializingAgents: (
    serverId: string,
    state: SessionFieldUpdate<"initializingAgents">,
  ) => void;

  // Agents
  setAgents: (serverId: string, agents: SessionFieldUpdate<"agents">) => void;
  setAgentDetails: (serverId: string, agents: SessionFieldUpdate<"agentDetails">) => void;
  setWorkspaces: (serverId: string, workspaces: SessionFieldUpdate<"workspaces">) => void;
  mergeWorkspaces: (serverId: string, workspaces: Iterable<WorkspaceDescriptor>) => void;
  removeWorkspace: (serverId: string, workspaceId: string) => void;
  setProjects: (serverId: string, projects: Iterable<ProjectDescriptor>) => void;
  upsertProject: (serverId: string, project: ProjectDescriptor) => void;
  removeProject: (serverId: string, projectId: string) => void;

  // Agent activity timestamps
  setAgentLastActivity: (agentId: string, timestamp: Date) => void;
  setAgentLastActivityBatch: (
    updates: Map<string, Date> | ((prev: Map<string, Date>) => Map<string, Date>),
  ) => void;
  flushAgentLastActivity: () => void;

  // Permissions
  setPendingPermissions: (
    serverId: string,
    perms: SessionFieldUpdate<"pendingPermissions">,
  ) => void;

  // File explorer
  setFileExplorer: (serverId: string, state: SessionFieldUpdate<"fileExplorer">) => void;

  // Queued messages
  setQueuedMessages: (serverId: string, value: SessionFieldUpdate<"queuedMessages">) => void;

  // Hydration
  setHasHydratedAgents: (serverId: string, hydrated: boolean) => void;
  setHasHydratedWorkspaces: (serverId: string, hydrated: boolean) => void;
  setHasWorkspaceDirectorySnapshot: (serverId: string, available: boolean) => void;
}

type SessionStore = SessionStoreState & SessionStoreActions;

const agentLastActivityCoalescer = createAgentLastActivityCoalescer();

// Helper to create initial session state
function createInitialSessionState(
  serverId: string,
  client: DaemonClient | null,
  clientGeneration = 0,
): SessionState {
  return {
    serverId,
    client,
    clientGeneration,
    viewedTimelineSync: null,
    serverInfo: null,
    hasHydratedAgents: false,
    hasHydratedWorkspaces: false,
    hasWorkspaceDirectorySnapshot: false,
    focusedAgentId: null,
    focusedTerminalId: null,
    agentStreamTail: new Map(),
    agentStreamHead: new Map(),
    agentTasks: new Map(),
    agentTaskCursor: new Map(),
    messageSubmissions: new Map(),
    agentTimelineCursor: new Map(),
    agentTimelineHasOlder: new Map(),
    agentTimelineHasNewer: new Map(),
    agentTimelineOlderFetchInFlight: new Map(),
    historySyncGeneration: 0,
    agentHistorySyncGeneration: new Map(),
    agentAuthoritativeHistoryApplied: new Map(),
    initializingAgents: new Map(),
    agents: new Map(),
    workspaceAgentActivity: new Map(),
    agentDetails: new Map(),
    workspaces: new Map(),
    projects: new Map(),
    pendingPermissions: new Map(),
    fileExplorer: new Map(),
    queuedMessages: new Map(),
  };
}

type SessionFieldUpdate<K extends keyof SessionState> =
  | SessionState[K]
  | ((current: SessionState[K]) => SessionState[K]);

// Replaces one field of one session, keeping the store identity when the value is unchanged.
function updateSessionField<K extends keyof SessionState>(
  prev: SessionStore,
  serverId: string,
  key: K,
  update: SessionFieldUpdate<K>,
): SessionStore {
  const session = prev.sessions[serverId];
  if (!session) {
    return prev;
  }
  const next =
    typeof update === "function"
      ? (update as (current: SessionState[K]) => SessionState[K])(session[key])
      : update;
  if (session[key] === next) {
    return prev;
  }
  return {
    ...prev,
    sessions: { ...prev.sessions, [serverId]: { ...session, [key]: next } },
  };
}

function areJsonEqual(current: unknown, next: unknown): boolean {
  return JSON.stringify(current ?? null) === JSON.stringify(next ?? null);
}

function isSessionServerInfoUnchanged(input: {
  currentServerInfo: SessionState["serverInfo"] | undefined;
  nextHostname: string | null;
  nextVersion: string | null;
  nextDesktopManaged: boolean | undefined;
  nextCapabilities: ServerCapabilities | undefined;
  nextFeatures: ServerInfoStatusPayload["features"] | undefined;
  nextServerId: string;
}): boolean {
  const {
    currentServerInfo,
    nextHostname,
    nextVersion,
    nextDesktopManaged,
    nextCapabilities,
    nextFeatures,
  } = input;
  const prevHostname = currentServerInfo?.hostname?.trim() || null;
  const prevVersion = currentServerInfo?.version?.trim() || null;
  return (
    currentServerInfo?.serverId === input.nextServerId &&
    prevHostname === nextHostname &&
    prevVersion === nextVersion &&
    currentServerInfo?.desktopManaged === nextDesktopManaged &&
    areJsonEqual(currentServerInfo?.capabilities, nextCapabilities) &&
    areJsonEqual(currentServerInfo?.features, nextFeatures)
  );
}

export const useSessionStore = create<SessionStore>()(
  subscribeWithSelector((set, get) => {
    const commitActivityUpdates: AgentLastActivityCommitter = (updates) => {
      set((prev) => {
        let nextActivity: Map<string, Date> | null = null;
        for (const [agentId, timestamp] of updates.entries()) {
          const current = prev.agentLastActivity.get(agentId);
          if (current && current.getTime() >= timestamp.getTime()) {
            continue;
          }
          if (!nextActivity) {
            nextActivity = new Map(prev.agentLastActivity);
          }
          nextActivity.set(agentId, timestamp);
        }
        if (!nextActivity) {
          return prev;
        }
        return {
          ...prev,
          agentLastActivity: nextActivity,
        };
      });
    };
    agentLastActivityCoalescer.setCommitter(commitActivityUpdates);

    return {
      sessions: {},
      agentLastActivity: new Map(),

      // Session management
      initializeSession: (serverId, client, clientGeneration) => {
        set((prev) => {
          if (prev.sessions[serverId]) {
            return prev;
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: createInitialSessionState(serverId, client, clientGeneration),
            },
          };
        });
      },

      clearSession: (serverId) => {
        agentLastActivityCoalescer.flushNow();
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          const nextSessions = { ...prev.sessions };
          delete nextSessions[serverId];
          let nextActivity = prev.agentLastActivity;
          if (session.agents.size > 0 || session.agentDetails.size > 0) {
            const candidate = new Map(prev.agentLastActivity);
            let changed = false;
            for (const agentId of new Set([
              ...session.agents.keys(),
              ...session.agentDetails.keys(),
            ])) {
              if (candidate.delete(agentId)) {
                changed = true;
              }
              agentLastActivityCoalescer.deletePending(agentId);
            }
            if (changed) {
              nextActivity = candidate;
            }
          }
          return {
            ...prev,
            sessions: nextSessions,
            agentLastActivity: nextActivity,
          };
        });
      },

      updateSessionClient: (serverId, client, clientGeneration = 0) => {
        set((prev) => {
          const session = prev.sessions[serverId];

          if (!session) {
            return prev;
          }

          if (session.client === client && session.clientGeneration === clientGeneration) {
            return prev;
          }

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                client,
                clientGeneration,
              },
            },
          };
        });
      },

      setViewedTimelineSync: (serverId, viewedTimelineSync) => {
        set((prev) => updateSessionField(prev, serverId, "viewedTimelineSync", viewedTimelineSync));
      },

      updateSessionServerInfo: (serverId, info) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }

          const nextHostname = info.hostname?.trim() || null;
          const nextVersion = info.version?.trim() || null;
          const nextDesktopManaged = info.desktopManaged;
          const nextCapabilities = info.capabilities;
          const nextFeatures = info.features;

          if (
            isSessionServerInfoUnchanged({
              currentServerInfo: session.serverInfo,
              nextHostname,
              nextVersion,
              nextDesktopManaged,
              nextCapabilities,
              nextFeatures,
              nextServerId: info.serverId,
            })
          ) {
            return prev;
          }

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                serverInfo: {
                  serverId: info.serverId,
                  hostname: nextHostname,
                  version: nextVersion,
                  ...(nextDesktopManaged !== undefined
                    ? { desktopManaged: nextDesktopManaged }
                    : {}),
                  ...(nextCapabilities ? { capabilities: nextCapabilities } : {}),
                  ...(nextFeatures ? { features: nextFeatures } : {}),
                },
              },
            },
          };
        });
      },

      getSession: (serverId) => {
        return get().sessions[serverId];
      },

      // Focus
      setFocusedAgentId: (serverId, agentId) => {
        set((prev) => updateSessionField(prev, serverId, "focusedAgentId", agentId));
      },

      setFocusedTerminalId: (serverId, terminalId) => {
        set((prev) => updateSessionField(prev, serverId, "focusedTerminalId", terminalId));
      },

      // Stream state (head/tail model)
      setAgentStreamTail: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "agentStreamTail", state));
      },

      setAgentStreamState: (serverId, agentId, state) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }

          let nextTail = session.agentStreamTail;
          let nextHead = session.agentStreamHead;
          let changedTail = false;
          let changedHead = false;

          if (state.tail !== undefined) {
            const existingTail = session.agentStreamTail.get(agentId);
            if (existingTail !== state.tail) {
              nextTail = new Map(session.agentStreamTail);
              nextTail.set(agentId, state.tail);
              changedTail = true;
            }
          }

          if (state.head !== undefined) {
            const existingHead = session.agentStreamHead.get(agentId);
            const shouldDeleteHead = state.head.length === 0;
            if (shouldDeleteHead) {
              if (session.agentStreamHead.has(agentId)) {
                nextHead = new Map(session.agentStreamHead);
                nextHead.delete(agentId);
                changedHead = true;
              }
            } else if (existingHead !== state.head) {
              nextHead = new Map(session.agentStreamHead);
              nextHead.set(agentId, state.head);
              changedHead = true;
            }
          }

          const currentSubmissions = session.messageSubmissions.get(agentId) ?? [];
          const observedSubmissions = observeMessageSubmissionCanonical(
            currentSubmissions,
            state.acknowledgedClientMessageIds ?? [],
          );
          const changedSubmissions = observedSubmissions !== currentSubmissions;
          const { agentTasks, agentTaskCursor } = updateAgentTaskSnapshot(
            session,
            agentId,
            state.taskSnapshot,
            state.taskSnapshotCursor,
          );
          const changedTasks = agentTasks !== session.agentTasks;
          const changedTaskCursor = agentTaskCursor !== session.agentTaskCursor;

          if (
            !changedTail &&
            !changedHead &&
            !changedSubmissions &&
            !changedTasks &&
            !changedTaskCursor
          ) {
            return prev;
          }

          let messageSubmissions = session.messageSubmissions;
          if (changedSubmissions) {
            messageSubmissions = new Map(session.messageSubmissions);
            if (observedSubmissions.length > 0) {
              messageSubmissions.set(agentId, observedSubmissions);
            } else {
              messageSubmissions.delete(agentId);
            }
          }

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentStreamTail: nextTail,
                agentStreamHead: nextHead,
                agentTasks,
                agentTaskCursor,
                messageSubmissions,
              },
            },
          };
        });
      },

      beginAgentMessageSubmission: (serverId, agentId, message) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) return prev;
          if (!message.clientMessageId) {
            throw new Error("Beginning a message submission requires client identity");
          }
          const currentTail = session.agentStreamTail.get(agentId) ?? [];
          const currentHead = session.agentStreamHead.get(agentId) ?? [];
          const stream = appendSubmittedUserMessage({
            tail: currentTail,
            head: currentHead,
            message,
          });
          const submissions = beginMessageSubmission(
            session.messageSubmissions.get(agentId) ?? [],
            { clientMessageId: message.clientMessageId },
          );
          const messageSubmissions = new Map(session.messageSubmissions);
          messageSubmissions.set(agentId, submissions);
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentStreamTail:
                  stream.tail === currentTail
                    ? session.agentStreamTail
                    : new Map(session.agentStreamTail).set(agentId, stream.tail),
                agentStreamHead:
                  stream.head === currentHead
                    ? session.agentStreamHead
                    : new Map(session.agentStreamHead).set(agentId, stream.head),
                messageSubmissions,
              },
            },
          };
        });
      },

      acceptAgentMessageSubmission: (serverId, agentId, clientMessageId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) return prev;
          const currentSubmissions = session.messageSubmissions.get(agentId) ?? [];
          const submissions = acceptMessageSubmission(currentSubmissions, clientMessageId);
          if (submissions === currentSubmissions) return prev;
          const messageSubmissions = new Map(session.messageSubmissions);
          if (submissions.length > 0) {
            messageSubmissions.set(agentId, submissions);
          } else {
            messageSubmissions.delete(agentId);
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, messageSubmissions },
            },
          };
        });
      },

      rejectAgentMessageSubmission: (serverId, agentId, clientMessageId) => {
        let outcome: MessageSubmissionRejectionOutcome = "unknown";
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) return prev;
          const currentTail = session.agentStreamTail.get(agentId) ?? [];
          const currentHead = session.agentStreamHead.get(agentId) ?? [];
          const currentSubmissions = session.messageSubmissions.get(agentId) ?? [];
          const result = rejectMessageSubmission(currentSubmissions, clientMessageId);
          outcome = result.outcome;
          if (outcome === "unknown") return prev;
          const stream =
            outcome === "rejected"
              ? removeSubmittedUserMessage({
                  tail: currentTail,
                  head: currentHead,
                  clientMessageId,
                })
              : { tail: currentTail, head: currentHead };
          const messageSubmissions = new Map(session.messageSubmissions);
          if (result.submissions.length > 0) {
            messageSubmissions.set(agentId, result.submissions);
          } else {
            messageSubmissions.delete(agentId);
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentStreamTail:
                  stream.tail === currentTail
                    ? session.agentStreamTail
                    : new Map(session.agentStreamTail).set(agentId, stream.tail),
                agentStreamHead:
                  stream.head === currentHead
                    ? session.agentStreamHead
                    : new Map(session.agentStreamHead).set(agentId, stream.head),
                messageSubmissions,
              },
            },
          };
        });
        return outcome;
      },

      handoffCreatedAgentUserMessage: (serverId, agentId, message) => {
        let didHandoff = false;
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }

          const currentTail = session.agentStreamTail.get(agentId) ?? [];
          const currentHead = session.agentStreamHead.get(agentId) ?? [];
          const result = handoffCreatedAgentUserMessageToStream({
            tail: currentTail,
            head: currentHead,
            message,
          });
          if (!result.changedTail && !result.changedHead) {
            return prev;
          }

          const nextTail = result.changedTail
            ? new Map(session.agentStreamTail).set(agentId, result.tail)
            : session.agentStreamTail;
          const nextHead = result.changedHead
            ? new Map(session.agentStreamHead).set(agentId, result.head)
            : session.agentStreamHead;
          didHandoff = true;

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentStreamTail: nextTail,
                agentStreamHead: nextHead,
              },
            },
          };
        });
        return didHandoff;
      },

      clearAgentStreamHead: (serverId, agentId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          if (!session.agentStreamHead.has(agentId)) {
            return prev;
          }
          const nextHead = new Map(session.agentStreamHead);
          nextHead.delete(agentId);
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, agentStreamHead: nextHead },
            },
          };
        });
      },

      setAgentTimelineCursor: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "agentTimelineCursor", state));
      },

      setAgentTimelineHasNewer: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "agentTimelineHasNewer", state));
      },

      setAgentTimelineOlderFetchInFlight: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "agentTimelineOlderFetchInFlight", state));
      },

      bumpHistorySyncGeneration: (serverId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          const nextGeneration = session.historySyncGeneration + 1;
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                historySyncGeneration: nextGeneration,
              },
            },
          };
        });
      },

      markAgentHistorySynchronized: (serverId, agentId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          const currentGeneration = session.historySyncGeneration;
          const previousGeneration = session.agentHistorySyncGeneration.get(agentId);
          if (previousGeneration === currentGeneration) {
            return prev;
          }
          const nextMap = new Map(session.agentHistorySyncGeneration);
          nextMap.set(agentId, currentGeneration);
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentHistorySyncGeneration: nextMap,
              },
            },
          };
        });
      },

      setAgentAuthoritativeHistoryApplied: (serverId, agentId, applied) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }

          const previousApplied = session.agentAuthoritativeHistoryApplied.get(agentId) ?? false;
          if (previousApplied === applied) {
            return prev;
          }

          const nextApplied = new Map(session.agentAuthoritativeHistoryApplied);
          if (applied) {
            nextApplied.set(agentId, true);
          } else {
            nextApplied.delete(agentId);
          }

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentAuthoritativeHistoryApplied: nextApplied,
              },
            },
          };
        });
      },

      applyAgentTimelineResponseState: (serverId, agentId, state) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) return prev;

          const nextTail = new Map(session.agentStreamTail);
          nextTail.set(agentId, state.items);
          const nextHead = new Map(session.agentStreamHead);
          if (state.head.length > 0) nextHead.set(agentId, state.head);
          else nextHead.delete(agentId);
          const nextCursor = new Map(session.agentTimelineCursor);
          if (state.range) nextCursor.set(agentId, state.range);
          else nextCursor.delete(agentId);
          const nextHasOlder = new Map(session.agentTimelineHasOlder);
          if (state.older !== "unchanged") {
            nextHasOlder.set(agentId, state.older === "available");
          }
          const nextHasNewer = new Map(session.agentTimelineHasNewer);
          nextHasNewer.set(agentId, state.newer);
          const nextAuthoritative = new Map(session.agentAuthoritativeHistoryApplied);
          const nextSyncGeneration = new Map(session.agentHistorySyncGeneration);
          const currentSubmissions = session.messageSubmissions.get(agentId) ?? [];
          const observedSubmissions = observeMessageSubmissionCanonical(
            currentSubmissions,
            state.acknowledgedClientMessageIds,
          );
          let messageSubmissions = session.messageSubmissions;
          if (observedSubmissions !== currentSubmissions) {
            messageSubmissions = new Map(session.messageSubmissions);
            if (observedSubmissions.length > 0) {
              messageSubmissions.set(agentId, observedSubmissions);
            } else {
              messageSubmissions.delete(agentId);
            }
          }
          if (state.synchronized) {
            nextAuthoritative.set(agentId, true);
            nextSyncGeneration.set(agentId, session.historySyncGeneration);
          }
          const epochChanged =
            state.range?.epoch !== session.agentTimelineCursor.get(agentId)?.epoch;
          // Historical windows cannot establish the current checklist. A new epoch still
          // invalidates the prior snapshot, even when its latest history is not loaded yet.
          const latestTask = [...state.items, ...state.head].findLast(
            (item) => item.kind === "todo_list",
          );
          const inferredTaskSnapshot = state.newer
            ? epochChanged
              ? []
              : undefined
            : latestTask?.kind === "todo_list"
              ? latestTask.items
              : state.older === "none" || epochChanged
                ? []
                : undefined;
          const taskSnapshot = state.taskSnapshot ?? inferredTaskSnapshot;
          const taskSnapshotCursor =
            state.taskSnapshot !== undefined
              ? state.taskSnapshotCursor
              : ((latestTask?.kind === "todo_list" ? latestTask.timelineCursor : undefined) ??
                (state.range && inferredTaskSnapshot !== undefined
                  ? { epoch: state.range.epoch, seq: state.range.endSeq }
                  : undefined));
          const { agentTasks, agentTaskCursor } = updateAgentTaskSnapshot(
            session,
            agentId,
            taskSnapshot,
            taskSnapshotCursor,
          );

          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agentStreamTail: nextTail,
                agentStreamHead: nextHead,
                agentTasks,
                agentTaskCursor,
                agentTimelineCursor: nextCursor,
                agentTimelineHasOlder: nextHasOlder,
                agentTimelineHasNewer: nextHasNewer,
                agentAuthoritativeHistoryApplied: nextAuthoritative,
                agentHistorySyncGeneration: nextSyncGeneration,
                messageSubmissions,
              },
            },
          };
        });
      },

      // Initializing agents
      setInitializingAgents: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "initializingAgents", state));
      },

      // Agents
      setAgents: (serverId, agents) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          const nextAgents = typeof agents === "function" ? agents(session.agents) : agents;
          if (session.agents === nextAgents) {
            return prev;
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: {
                ...session,
                agents: nextAgents,
                workspaceAgentActivity: buildWorkspaceAgentActivityIndex(
                  nextAgents,
                  session.workspaceAgentActivity,
                ),
              },
            },
          };
        });
      },

      setAgentDetails: (serverId, agents) => {
        set((prev) => updateSessionField(prev, serverId, "agentDetails", agents));
      },

      setWorkspaces: (serverId, workspaces) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) {
            return prev;
          }
          const nextWorkspaces =
            typeof workspaces === "function" ? workspaces(session.workspaces) : workspaces;
          const preservedWorkspaces = preserveMapIdentity(session.workspaces, nextWorkspaces);
          if (session.workspaces === preservedWorkspaces) {
            return prev;
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, workspaces: preservedWorkspaces },
            },
          };
        });
      },

      setProjects: (serverId, projects) => {
        const next = new Map<string, ProjectDescriptor>();
        for (const project of projects) next.set(project.projectId, project);
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session) return prev;
          const preservedProjects = preserveMapIdentity(session.projects, next);
          if (session.projects === preservedProjects) return prev;
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, projects: preservedProjects },
            },
          };
        });
      },

      upsertProject: (serverId, project) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session || equal(session.projects.get(project.projectId), project)) return prev;
          const projects = new Map(session.projects);
          projects.set(project.projectId, project);
          return {
            ...prev,
            sessions: { ...prev.sessions, [serverId]: { ...session, projects } },
          };
        });
      },

      removeProject: (serverId, projectId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session?.projects.has(projectId)) return prev;
          const projects = new Map(session.projects);
          projects.delete(projectId);
          return {
            ...prev,
            sessions: { ...prev.sessions, [serverId]: { ...session, projects } },
          };
        });
      },

      mergeWorkspaces: (serverId, workspaces) => {
        const nextEntries = Array.from(workspaces);
        set((prev) => {
          const session = prev.sessions[serverId];
          if (!session || nextEntries.length === 0) {
            return prev;
          }
          const next = new Map(session.workspaces);
          let changed = false;
          for (const workspace of nextEntries) {
            const existing = next.get(workspace.id);
            const nextWorkspace = preserveWorkspaceDescriptorIdentity(workspace, existing);
            if (existing === nextWorkspace) {
              continue;
            }
            next.set(workspace.id, nextWorkspace);
            changed = true;
          }
          if (!changed) {
            return prev;
          }
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, workspaces: next },
            },
          };
        });
      },

      removeWorkspace: (serverId, workspaceId) => {
        set((prev) => {
          const session = prev.sessions[serverId];
          const workspaceKey = resolveWorkspaceMapKeyByIdentity({
            workspaces: session?.workspaces,
            workspaceId,
          });
          if (!session || !workspaceKey) {
            return prev;
          }
          const next = new Map(session.workspaces);
          next.delete(workspaceKey);
          return {
            ...prev,
            sessions: {
              ...prev.sessions,
              [serverId]: { ...session, workspaces: next },
            },
          };
        });
      },

      // Agent activity timestamps (top-level, does NOT mutate session object)
      setAgentLastActivity: (agentId, timestamp) => {
        agentLastActivityCoalescer.enqueue(agentId, timestamp);
      },

      setAgentLastActivityBatch: (updates) => {
        set((prev) => {
          const nextActivity =
            typeof updates === "function" ? updates(prev.agentLastActivity) : updates;
          if (nextActivity === prev.agentLastActivity) {
            return prev;
          }
          if (nextActivity.size === 0) {
            if (prev.agentLastActivity.size === 0) {
              return prev;
            }
            return {
              ...prev,
              agentLastActivity: new Map(),
            };
          }
          let changed = false;
          for (const [agentId, timestamp] of nextActivity.entries()) {
            const currentTimestamp = prev.agentLastActivity.get(agentId);
            if (!currentTimestamp || currentTimestamp.getTime() !== timestamp.getTime()) {
              changed = true;
              break;
            }
          }
          if (!changed && nextActivity.size === prev.agentLastActivity.size) {
            return prev;
          }
          return {
            ...prev,
            agentLastActivity: new Map(nextActivity),
          };
        });
      },

      flushAgentLastActivity: () => {
        agentLastActivityCoalescer.flushNow();
      },

      // Permissions
      setPendingPermissions: (serverId, perms) => {
        set((prev) => updateSessionField(prev, serverId, "pendingPermissions", perms));
      },

      // File explorer
      setFileExplorer: (serverId, state) => {
        set((prev) => updateSessionField(prev, serverId, "fileExplorer", state));
      },

      // Queued messages
      setQueuedMessages: (serverId, value) => {
        set((prev) => updateSessionField(prev, serverId, "queuedMessages", value));
      },

      // Hydration
      setHasHydratedAgents: (serverId, hydrated) => {
        set((prev) => updateSessionField(prev, serverId, "hasHydratedAgents", hydrated));
      },

      setHasHydratedWorkspaces: (serverId, hydrated) => {
        set((prev) => updateSessionField(prev, serverId, "hasHydratedWorkspaces", hydrated));
      },

      setHasWorkspaceDirectorySnapshot: (serverId, available) => {
        set((prev) =>
          updateSessionField(prev, serverId, "hasWorkspaceDirectorySnapshot", available),
        );
      },
    };
  }),
);
