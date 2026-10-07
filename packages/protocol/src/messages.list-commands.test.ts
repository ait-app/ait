import { describe, expect, test } from "vitest";
import { SessionInboundMessageSchema, SessionOutboundMessageSchema } from "./messages.js";

describe("agent.commands.list.request schema", () => {
  test("accepts legacy agent-only payload", () => {
    const parsed = SessionInboundMessageSchema.parse({
      type: "agent.commands.list.request",
      agentId: "agent-123",
      requestId: "req-123",
    });

    expect(parsed.type).toBe("agent.commands.list.request");
    if (parsed.type !== "agent.commands.list.request") {
      throw new Error("Expected agent.commands.list.request message");
    }
    expect(parsed.agentId).toBe("agent-123");
    expect(parsed.draftConfig).toBeUndefined();
  });

  test("accepts draft command context payload", () => {
    const parsed = SessionInboundMessageSchema.parse({
      type: "agent.commands.list.request",
      agentId: "__new_agent__",
      draftConfig: {
        provider: "codex",
        cwd: "/tmp/project",
        modeId: "bypassPermissions",
        model: "gpt-5",
        thinkingOptionId: "off",
        featureValues: {
          plan_mode: true,
        },
      },
      requestId: "req-456",
    });

    expect(parsed.type).toBe("agent.commands.list.request");
    if (parsed.type !== "agent.commands.list.request") {
      throw new Error("Expected agent.commands.list.request message");
    }
    expect(parsed.draftConfig).toEqual({
      provider: "codex",
      cwd: "/tmp/project",
      modeId: "bypassPermissions",
      model: "gpt-5",
      thinkingOptionId: "off",
      featureValues: {
        plan_mode: true,
      },
    });
  });

  test("preserves command kind metadata in responses", () => {
    const parsed = SessionOutboundMessageSchema.parse({
      type: "agent.commands.list.response",
      payload: {
        agentId: "agent-123",
        requestId: "req-123",
        error: null,
        commands: [
          {
            name: "taste",
            description: "Apply code taste",
            argumentHint: "",
            kind: "skill",
          },
        ],
      },
    });

    expect(parsed.type).toBe("agent.commands.list.response");
    if (parsed.type !== "agent.commands.list.response") {
      throw new Error("Expected agent.commands.list.response message");
    }
    expect(parsed.payload.commands).toEqual([
      {
        name: "taste",
        description: "Apply code taste",
        argumentHint: "",
        kind: "skill",
      },
    ]);
  });

  test("falls back to command for unknown future command kinds", () => {
    const parsed = SessionOutboundMessageSchema.parse({
      type: "agent.commands.list.response",
      payload: {
        agentId: "agent-123",
        requestId: "req-123",
        error: null,
        commands: [
          {
            name: "future-command",
            description: "Future command kind",
            argumentHint: "",
            kind: "future-kind",
          },
        ],
      },
    });

    expect(parsed.type).toBe("agent.commands.list.response");
    if (parsed.type !== "agent.commands.list.response") {
      throw new Error("Expected agent.commands.list.response message");
    }
    expect(parsed.payload.commands[0]?.kind).toBe("command");
  });
});
