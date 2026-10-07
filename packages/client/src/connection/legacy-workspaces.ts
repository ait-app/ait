import { AgentSnapshotPayloadSchema } from "@ait/protocol/messages";
import type { AgentSnapshotPayload, SessionOutboundMessage } from "@ait/protocol/messages";
import {
  deriveAgentStateBucket,
  getWorkspaceStateBucketPriority,
} from "@ait/protocol/agent-state-bucket";

type AgentEntry = Extract<
  SessionOutboundMessage,
  { type: "agent.list.response" }
>["payload"]["entries"][number];
type Workspace = Extract<
  SessionOutboundMessage,
  { type: "workspace.list.response" }
>["payload"]["entries"][number];
type WorkspaceUpdate = Extract<SessionOutboundMessage, { type: "workspace.update" }>;

// Preserve the pre-registry app's identity format. This is an opaque ID, never
// a filesystem path: converting C:\ to C: changes a drive root into a relative path.
function legacyWorkspaceId(value: string): string {
  return value.trim().replace(/\\/g, "/").replace(/\/+$/, "") || "/";
}

// COMPAT(legacyWorkspaceDaemon): restored in v0.8.0; remove after 2027-03-11 once daemon floor >= v0.1.97.
// The pre-registry app grouped agents by checkout path. Keep that representation
// at the client edge, including live updates, so app workflows need no version branch.
export class LegacyWorkspaces {
  private readonly agents = new Map<string, AgentEntry>();

  normalize(message: SessionOutboundMessage): SessionOutboundMessage {
    const stamp = (agent: AgentSnapshotPayload): AgentSnapshotPayload => ({
      ...agent,
      workspaceId: agent.workspaceId ?? legacyWorkspaceId(agent.cwd),
    });
    if (message.type === "agent.list.response")
      return {
        ...message,
        payload: {
          ...message.payload,
          entries: message.payload.entries.map((entry) => ({
            ...entry,
            agent: stamp(entry.agent),
          })),
        },
      };
    if (message.type === "agent.update" && message.payload.kind === "upsert")
      return {
        ...message,
        payload: { ...message.payload, agent: stamp(message.payload.agent) },
      };
    if (
      (message.type === "agent.get.response" ||
        message.type === "agent.timeline.get.response" ||
        message.type === "agent.cancel.response") &&
      message.payload.agent
    ) {
      return {
        ...message,
        payload: { ...message.payload, agent: stamp(message.payload.agent) },
      } as SessionOutboundMessage;
    }
    if (
      message.type === "status" &&
      (message.payload.status === "agent_created" || message.payload.status === "agent_resumed")
    ) {
      return {
        ...message,
        payload: {
          ...message.payload,
          agent: stamp(AgentSnapshotPayloadSchema.parse(message.payload.agent)),
        },
      };
    }
    return message;
  }

  read(entries: AgentEntry[], reset: boolean): Workspace[] {
    if (reset) this.agents.clear();
    for (const entry of entries) this.agents.set(entry.agent.id, entry);
    const pageIds = new Set(entries.map(workspaceId));
    return [...this.workspaces().values()].filter((workspace) => pageIds.has(workspace.id));
  }

  update(message: SessionOutboundMessage): WorkspaceUpdate[] {
    if (message.type !== "agent.update") return [];
    const before = this.workspaces();
    const update = message.payload;
    if (update.kind === "remove") this.agents.delete(update.agentId);
    else {
      const project = update.project ?? this.agents.get(update.agent.id)?.project;
      if (update.agent.archivedAt) this.agents.delete(update.agent.id);
      else if (project) this.agents.set(update.agent.id, { agent: update.agent, project });
    }
    const after = this.workspaces();
    const changes: WorkspaceUpdate[] = [];
    for (const [id, workspace] of after) {
      if (JSON.stringify(before.get(id)) !== JSON.stringify(workspace))
        changes.push({ type: "workspace.update", payload: { kind: "upsert", workspace } });
    }
    for (const id of before.keys())
      if (!after.has(id))
        changes.push({ type: "workspace.update", payload: { kind: "remove", id } });
    return changes;
  }

  private workspaces(): Map<string, Workspace> {
    const workspaces = new Map<string, Workspace>();
    for (const entry of this.agents.values()) {
      const { agent, project } = entry;
      const { checkout } = project;
      const id = workspaceId(entry);
      const status = deriveAgentStateBucket({
        status: agent.status,
        pendingPermissionCount: agent.pendingPermissions.length,
        requiresAttention: agent.requiresAttention,
        attentionReason: agent.attentionReason,
      });
      const existing = workspaces.get(id);
      if (
        existing &&
        getWorkspaceStateBucketPriority(existing.status) <= getWorkspaceStateBucketPriority(status)
      )
        continue;
      workspaces.set(id, {
        id,
        projectId: project.projectKey,
        projectDisplayName: project.projectName,
        projectCustomName: null,
        projectRootPath: checkout.mainRepoRoot ?? checkout.worktreeRoot ?? checkout.cwd,
        workspaceDirectory: checkout.cwd,
        projectKind: checkout.isGit ? "git" : "non_git",
        workspaceKind: workspaceKind(checkout),
        name: workspaceName(entry, id),
        title: null,
        status,
        statusEnteredAt: agent.attentionTimestamp ?? agent.updatedAt,
        activityAt: agent.updatedAt,
        archivingAt: null,
        diffStat: null,
        scripts: [],
        gitRuntime: gitRuntime(checkout),
        githubRuntime: null,
        project,
      });
    }
    return workspaces;
  }
}

function workspaceId(entry: AgentEntry): string {
  return entry.agent.workspaceId ?? legacyWorkspaceId(entry.project.checkout.cwd);
}

function workspaceKind(checkout: AgentEntry["project"]["checkout"]): Workspace["workspaceKind"] {
  if (!checkout.isGit) return "directory";
  if (checkout.isPaseoOwnedWorktree) return "worktree";
  return "checkout";
}

function workspaceName(entry: AgentEntry, id: string): string {
  const name = entry.project.workspaceName?.trim();
  if (name) return name;
  const branch = entry.project.checkout.currentBranch?.trim();
  if (branch && branch !== "HEAD") return branch;
  return id.slice(id.lastIndexOf("/") + 1);
}

function gitRuntime(checkout: AgentEntry["project"]["checkout"]): Workspace["gitRuntime"] {
  if (!checkout.isGit) return null;
  return {
    currentBranch: checkout.currentBranch,
    remoteUrl: checkout.remoteUrl,
    isPaseoOwnedWorktree: checkout.isPaseoOwnedWorktree,
    isDirty: null,
    aheadBehind: null,
    aheadOfOrigin: null,
    behindOfOrigin: null,
  };
}
