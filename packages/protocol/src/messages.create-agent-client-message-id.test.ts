import { describe, expect, it } from "vitest";
import { SessionInboundMessageSchema } from "./messages";
import { MAX_EXPLICIT_AGENT_TITLE_CHARS } from "@ait/protocol/agent-title-limits";

describe("agent.create.request clientMessageId", () => {
  it("accepts clientMessageId for stable initial prompt transfer", () => {
    const parsed = SessionInboundMessageSchema.parse({
      type: "agent.create.request",
      requestId: "req-1",
      clientMessageId: "client-msg-1",
      config: {
        provider: "claude",
        cwd: "/tmp/project",
      },
      initialPrompt: "hello",
    });

    expect(parsed.type).toBe("agent.create.request");
    if (parsed.type !== "agent.create.request") {
      throw new Error("Expected agent.create.request");
    }
    expect(parsed.clientMessageId).toBe("client-msg-1");
  });

  it("accepts explicit titles up to the create-agent limit", () => {
    const parsed = SessionInboundMessageSchema.parse({
      type: "agent.create.request",
      requestId: "req-title-ok",
      config: {
        provider: "claude",
        cwd: "/tmp/project",
        title: "x".repeat(MAX_EXPLICIT_AGENT_TITLE_CHARS),
      },
    });

    expect(parsed.type).toBe("agent.create.request");
    if (parsed.type !== "agent.create.request") {
      throw new Error("Expected agent.create.request");
    }
    expect(parsed.config.title).toHaveLength(MAX_EXPLICIT_AGENT_TITLE_CHARS);
  });

  it("rejects explicit titles longer than the create-agent limit", () => {
    const parsed = SessionInboundMessageSchema.safeParse({
      type: "agent.create.request",
      requestId: "req-title-too-long",
      config: {
        provider: "claude",
        cwd: "/tmp/project",
        title: "x".repeat(MAX_EXPLICIT_AGENT_TITLE_CHARS + 1),
      },
    });

    expect(parsed.success).toBe(false);
  });
});
