import { describe, expect, it } from "vitest";
import { sessionEventKind, sessionEventMethod } from "./session-event-kinds.js";

describe("Ait methods and Rust session event kinds", () => {
  it.each([
    ["provider.snapshot.update", "providers_snapshot_update"],
    ["agent.attention.required", "agent_attention_required"],
    ["agent.permission.request", "agent_permission_request"],
    ["agent.permission.resolved", "agent_permission_resolved"],
    ["terminal.attention.required", "terminal_attention_required"],
    ["checkout.status.update", "checkout_status_update"],
  ])("preserves the subscription kind for %s", (method, kind) => {
    expect(sessionEventKind(method)).toBe(kind);
    expect(sessionEventMethod(kind)).toBe(method);
    expect(sessionEventMethod(method)).toBe(method);
  });

  it("preserves already canonical and unknown categories", () => {
    for (const method of ["project.update", "status.server_info", "future.event"]) {
      expect(sessionEventKind(method)).toBe(method);
      expect(sessionEventMethod(method)).toBe(method);
    }
  });
});
