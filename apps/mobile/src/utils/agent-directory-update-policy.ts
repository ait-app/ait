import equal from "fast-deep-equal";
import type { AgentUsage } from "@ait/protocol/agent-types";

interface AgentUpdateValue {
  updatedAt: Date | string;
  lastUsage?: AgentUsage;
  observation?: { revision?: number };
}

function timestamp(value: Date | string): number {
  return value instanceof Date ? value.getTime() : Date.parse(value);
}

export function acceptAgentDirectoryUpdate<T extends AgentUpdateValue>(
  current: T | undefined,
  incoming: T,
): T {
  if (current?.observation && incoming.observation) {
    const previousRevision = current.observation.revision ?? 0;
    const incomingRevision = incoming.observation.revision ?? 0;
    if (incomingRevision < previousRevision) return current;
    if (incomingRevision > previousRevision) return incoming;
  }
  if (!current || timestamp(incoming.updatedAt) >= timestamp(current.updatedAt)) return incoming;
  if (incoming.lastUsage === undefined) return current;
  if (equal(incoming.lastUsage, current.lastUsage)) return current;
  return { ...current, lastUsage: incoming.lastUsage };
}
