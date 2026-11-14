import type { TerminalState } from "@ait/protocol/messages";

interface WorkspaceTerminalSnapshots {
  get: (input: { terminalId: string }) => TerminalState | null;
  set: (input: { terminalId: string; state: TerminalState }) => void;
  clear: (input: { terminalId: string }) => void;
}

interface WorkspaceTerminalSession {
  snapshots: WorkspaceTerminalSnapshots;
}

const sessionsByScopeKey = new Map<string, WorkspaceTerminalSession>();
const refCountByScopeKey = new Map<string, number>();

function createSnapshots(): WorkspaceTerminalSnapshots {
  const snapshotByTerminalId = new Map<string, TerminalState>();
  return {
    get: ({ terminalId }) => snapshotByTerminalId.get(terminalId) ?? null,
    set: ({ terminalId, state }) => {
      snapshotByTerminalId.set(terminalId, state);
    },
    clear: ({ terminalId }) => {
      snapshotByTerminalId.delete(terminalId);
    },
  };
}

export function getWorkspaceTerminalSession(input: { scopeKey: string }): WorkspaceTerminalSession {
  const existing = sessionsByScopeKey.get(input.scopeKey);
  if (existing) {
    return existing;
  }

  const session: WorkspaceTerminalSession = { snapshots: createSnapshots() };
  sessionsByScopeKey.set(input.scopeKey, session);
  return session;
}

export function retainWorkspaceTerminalSession(input: { scopeKey: string }): void {
  const current = refCountByScopeKey.get(input.scopeKey) ?? 0;
  refCountByScopeKey.set(input.scopeKey, current + 1);
}

export function releaseWorkspaceTerminalSession(input: { scopeKey: string }): void {
  const current = refCountByScopeKey.get(input.scopeKey) ?? 0;
  if (current > 1) {
    refCountByScopeKey.set(input.scopeKey, current - 1);
    return;
  }
  refCountByScopeKey.delete(input.scopeKey);
  sessionsByScopeKey.delete(input.scopeKey);
}
