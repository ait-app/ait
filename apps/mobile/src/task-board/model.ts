import {
  deriveAgentStateBucket,
  getWorkspaceStateBucketPriority,
} from "@ait/protocol/agent-state-bucket";
import type { WorkspaceStateBucket } from "@ait/protocol/agent-state-bucket";
import type { Agent, WorkspaceDescriptor } from "@/stores/session-store";

export const TASK_LANES = ["needs_action", "running", "review", "done"] as const;
export type TaskLane = (typeof TASK_LANES)[number];
export type TaskSort = "recent" | "oldest";

export interface TaskBoardHost {
  serverId: string;
  label: string;
  agents: ReadonlyMap<string, Agent>;
  workspaces: ReadonlyMap<string, WorkspaceDescriptor>;
}

export interface BoardTask {
  key: string;
  agent: Agent;
  hostLabel: string;
  projectKey: string;
  projectName: string;
  workspaceName: string;
  ownBucket: WorkspaceStateBucket;
  bucket: WorkspaceStateBucket;
  activityAt: number;
  children: BoardTask[];
}

export interface TaskBoardModel {
  roots: BoardTask[];
  byKey: Map<string, BoardTask>;
}

export function taskKey(serverId: string, agentId: string): string {
  return JSON.stringify([serverId, agentId]);
}

function pathName(path: string): string {
  return (
    path
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .at(-1) || path
  );
}

export function taskLane(bucket: WorkspaceStateBucket): TaskLane {
  if (bucket === "needs_input" || bucket === "failed") return "needs_action";
  if (bucket === "running") return "running";
  return bucket === "attention" ? "review" : "done";
}

function ownBucket(agent: Agent): WorkspaceStateBucket {
  if (agent.observation?.status === "unknown") return "attention";
  return deriveAgentStateBucket({
    status:
      agent.status === "initializing" || (agent.turn.phase === "open" && agent.status !== "error")
        ? "running"
        : agent.status,
    pendingPermissionCount: agent.pendingPermissions.length,
    requiresAttention: agent.requiresAttention,
    attentionReason: agent.attentionReason,
  });
}

/** The board groups managed and observed children within the same host. */
export function buildTaskBoard(hosts: readonly TaskBoardHost[]): TaskBoardModel {
  const byKey = new Map<string, BoardTask>();
  for (const host of hosts) {
    for (const agent of host.agents.values()) {
      if (agent.archivedAt) continue;
      const workspace = agent.workspaceId ? host.workspaces.get(agent.workspaceId) : undefined;
      if (workspace?.archivingAt) continue;
      const projectName = workspace?.projectDisplayName || pathName(agent.cwd);
      const bucket = ownBucket(agent);
      const key = taskKey(host.serverId, agent.id);
      byKey.set(key, {
        key,
        agent,
        hostLabel: host.label,
        projectKey: taskKey(host.serverId, workspace?.projectId || agent.cwd),
        projectName,
        workspaceName: workspace?.title?.trim() || workspace?.name || pathName(agent.cwd),
        ownBucket: bucket,
        bucket,
        activityAt: agent.lastActivityAt.getTime(),
        children: [],
      });
    }
  }

  const roots: BoardTask[] = [];
  for (const task of byKey.values()) {
    const parent = task.agent.parentAgentId
      ? byKey.get(taskKey(task.agent.serverId, task.agent.parentAgentId))
      : undefined;
    // Corrupt/cyclic ancestry must not hide a task or recurse forever.
    let ancestor = parent;
    const visited = new Set([task.key]);
    while (ancestor && !visited.has(ancestor.key)) {
      visited.add(ancestor.key);
      ancestor = ancestor.agent.parentAgentId
        ? byKey.get(taskKey(ancestor.agent.serverId, ancestor.agent.parentAgentId))
        : undefined;
    }
    if (parent && !ancestor) parent.children.push(task);
    else roots.push(task);
  }

  function aggregate(task: BoardTask): void {
    for (const child of task.children) {
      aggregate(child);
      if (
        getWorkspaceStateBucketPriority(child.bucket) < getWorkspaceStateBucketPriority(task.bucket)
      ) {
        task.bucket = child.bucket;
      }
      task.activityAt = Math.max(task.activityAt, child.activityAt);
    }
    task.children.sort((a, b) => a.agent.createdAt.getTime() - b.agent.createdAt.getTime());
  }
  roots.forEach(aggregate);
  return { roots, byKey };
}

export function taskDescendants(task: BoardTask): BoardTask[] {
  return task.children.flatMap((child) => [child, ...taskDescendants(child)]);
}

export function taskActions(task: BoardTask) {
  const { agent } = task;
  const observed = agent.management === "observed" || Boolean(agent.observation);
  const writable = !observed && agent.capabilities.supportsMessageSend !== false;
  const active = [task, ...taskDescendants(task)].some(
    (member) =>
      member.ownBucket === "running" ||
      member.ownBucket === "needs_input" ||
      member.agent.observation?.status === "unknown",
  );
  return {
    approve: writable,
    stop:
      writable &&
      agent.capabilities.supportsCancel !== false &&
      (agent.status === "running" ||
        agent.status === "initializing" ||
        agent.turn.phase === "open"),
    read: task.ownBucket === "attention" && agent.requiresAttention,
    archive:
      !active &&
      (observed
        ? agent.capabilities.supportsArchive === true
        : agent.capabilities.supportsArchive !== false),
  };
}

export interface TaskFilters {
  hostId: string | null;
  projectKey: string | null;
  search: string;
  sort: TaskSort;
}

export function filterTaskBoard(tasks: readonly BoardTask[], filters: TaskFilters): BoardTask[] {
  const words = filters.search.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  return tasks
    .filter((task) => {
      if (filters.hostId && task.agent.serverId !== filters.hostId) return false;
      const family = [task, ...taskDescendants(task)];
      if (filters.projectKey && !family.some((member) => member.projectKey === filters.projectKey))
        return false;
      const haystack = family
        .map((member) =>
          [
            member.agent.title,
            member.agent.provider,
            member.agent.model,
            member.projectName,
            member.workspaceName,
            member.hostLabel,
            member.agent.cwd,
          ].join(" "),
        )
        .join(" ")
        .toLocaleLowerCase();
      return words.every((word) => haystack.includes(word));
    })
    .sort((a, b) => {
      const laneOrder =
        TASK_LANES.indexOf(taskLane(a.bucket)) - TASK_LANES.indexOf(taskLane(b.bucket));
      if (laneOrder) return laneOrder;
      if (taskLane(a.bucket) === "needs_action" && taskLane(b.bucket) === "needs_action") {
        const priority =
          getWorkspaceStateBucketPriority(a.bucket) - getWorkspaceStateBucketPriority(b.bucket);
        if (priority) return priority;
      }
      const byTime =
        filters.sort === "recent" ? b.activityAt - a.activityAt : a.activityAt - b.activityAt;
      return byTime || a.key.localeCompare(b.key);
    });
}
