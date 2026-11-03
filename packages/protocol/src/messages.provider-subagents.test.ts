import { describe, expect, test } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";

describe("provider subagent protocol", () => {
  test("preserves unified discovery, directory opt-in and observation facts", () => {
    expect(
      SessionInboundMessageSchema.parse({
        type: "agent.subagents.list.request",
        requestId: "request",
        parentAgentId: "parent",
        subagentId: "native",
      }),
    ).toMatchObject({ subagentId: "native" });
    expect(
      SessionInboundMessageSchema.parse({
        type: "agent.list.request",
        requestId: "request",
        includeObserved: true,
      }),
    ).toMatchObject({ includeObserved: true });
    const response = SessionOutboundMessageSchema.parse({
      type: "agent.subagents.list.response",
      payload: {
        requestId: "request",
        parentAgentId: "parent",
        error: null,
        agents: [
          {
            id: "observed",
            provider: "codex",
            management: "observed",
            parentAgentId: "parent",
            cwd: "/project",
            model: null,
            createdAt: "2026-10-31T00:00:00Z",
            updatedAt: "2026-10-31T00:00:00Z",
            lastUserMessageAt: null,
            status: "closed",
            capabilities: {
              supportsStreaming: true,
              supportsSessionPersistence: false,
              supportsDynamicModes: false,
              supportsMcpServers: false,
              supportsReasoningStream: false,
              supportsToolInvocations: true,
              supportsMessageSend: false,
            },
            currentModeId: null,
            availableModes: [],
            pendingPermissions: [],
            persistence: null,
            title: "Inspect project",
            labels: {},
            observation: {
              rootAgentId: "parent",
              nativeId: "native",
              status: "unknown",
              health: "stale",
              observedAt: "2026-10-31T00:00:00Z",
              description: "Inspect project",
              subtitle: "Explore",
            },
          },
        ],
      },
    });
    expect(response).toMatchObject({
      payload: {
        agents: [
          {
            management: "observed",
            parentAgentId: "parent",
            observation: { status: "unknown", health: "stale" },
          },
        ],
      },
    });
  });

  test("accepts a scoped timeline request and structured live update", () => {
    expect(
      SessionInboundMessageSchema.parse({
        type: "agent.provider_subagents.timeline.get.request",
        parentAgentId: "parent-1",
        subagentId: "child-1",
        requestId: "request-1",
      }),
    ).toMatchObject({ parentAgentId: "parent-1", subagentId: "child-1" });

    expect(
      SessionOutboundMessageSchema.parse({
        type: "agent.provider_subagents.update",
        payload: {
          kind: "timeline",
          parentAgentId: "parent-1",
          subagentId: "child-1",
          provider: "claude",
          epoch: "epoch-1",
          seq: 4,
          timestamp: "2026-07-12T10:00:00.000Z",
          item: { type: "assistant_message", text: "Found it." },
        },
      }),
    ).toMatchObject({
      payload: {
        kind: "timeline",
        parentAgentId: "parent-1",
        subagentId: "child-1",
        seq: 4,
      },
    });
  });

  test("accepts a provider child working directory while remaining compatible when absent", () => {
    const descriptor = {
      id: "child-1",
      parentAgentId: "parent-1",
      provider: "opencode",
      title: "Explore",
      description: null,
      status: "running",
      createdAt: "2026-07-12T10:00:00.000Z",
      updatedAt: "2026-07-12T10:00:00.000Z",
      toolCallId: null,
    };

    expect(
      SessionOutboundMessageSchema.parse({
        type: "agent.provider_subagents.list.response",
        payload: {
          requestId: "request-1",
          parentAgentId: "parent-1",
          subagents: [{ ...descriptor, cwd: "/workspace/child" }],
          error: null,
        },
      }),
    ).toMatchObject({ payload: { subagents: [{ cwd: "/workspace/child" }] } });

    expect(
      SessionOutboundMessageSchema.parse({
        type: "agent.provider_subagents.list.response",
        payload: {
          requestId: "request-2",
          parentAgentId: "parent-1",
          subagents: [descriptor],
          error: null,
        },
      }),
    ).toMatchObject({ payload: { subagents: [{ id: "child-1" }] } });
  });
});
