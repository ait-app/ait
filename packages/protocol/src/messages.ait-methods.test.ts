import { describe, expect, it } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";

describe("Ait operation names", () => {
  it.each([
    "fetch_agents_request",
    "create_agent_request",
    "project_icon_request",
    "start_workspace_script_request",
    "terminal_input",
    "dictation_stream_start",
    "schedule/list",
    "ping",
  ])("rejects the legacy inbound name %s", (type) => {
    expect(SessionInboundMessageSchema.safeParse({ type, requestId: "request" }).success).toBe(
      false,
    );
  });

  it("parses merged aliases through a single canonical discriminator", () => {
    expect(
      SessionInboundMessageSchema.parse({
        type: "project.icon.get.request",
        projectId: "project",
        requestId: "icon",
      }),
    ).toMatchObject({ projectId: "project" });
    expect(
      SessionInboundMessageSchema.parse({
        type: "workspace.script.start.request",
        workspaceId: "workspace",
        scriptName: "web",
        requestId: "script",
      }),
    ).toMatchObject({ scriptName: "web" });
    expect(
      SessionInboundMessageSchema.parse({
        type: "agent.create.request",
        config: { provider: "codex", cwd: "/repo" },
        requestId: "agent",
        subscribe: true,
      }),
    ).toMatchObject({ subscribe: true });
    expect(
      SessionOutboundMessageSchema.parse({
        type: "workspace.script.start.response",
        payload: {
          requestId: "script",
          workspaceId: "workspace",
          script: null,
          error: null,
        },
      }),
    ).toMatchObject({ payload: { script: null } });
  });
});
