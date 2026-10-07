// SessionEventKind identifiers are subscription parameters, separate from method names.
// Keep these aligned with crates/metadata/src/protocol/session.rs.
const SESSION_EVENT_KINDS: Readonly<Record<string, string>> = {
  "provider.snapshot.update": "providers_snapshot_update",
  "agent.attention.required": "agent_attention_required",
  "agent.permission.request": "agent_permission_request",
  "agent.permission.resolved": "agent_permission_resolved",
  "terminal.attention.required": "terminal_attention_required",
  "checkout.status.update": "checkout_status_update",
};

const SESSION_EVENT_METHODS = Object.fromEntries(
  Object.entries(SESSION_EVENT_KINDS).map(([method, kind]) => [kind, method]),
);

export function sessionEventKind(method: string): string {
  return SESSION_EVENT_KINDS[method] ?? method;
}

export function sessionEventMethod(kind: string): string {
  return SESSION_EVENT_METHODS[kind] ?? kind;
}
