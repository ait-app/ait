import { describe, expect, it } from "vitest";

import {
  AgentSnapshotPayloadSchema,
  SessionInboundMessageSchema,
  SessionOutboundMessageSchema,
  WSOutboundMessageSchema,
} from "./messages.js";
import {
  SIDE_QUESTION_LIMITS,
  SideAskRequestSchema,
  SideContextSnapshotSchema,
  SideQuestionsStateSchema,
  SideTopicSchema,
  SideTransferResponseMessageSchema,
  canFollowUp,
  latestSideTurn,
} from "./side-questions.js";

const topic = {
  id: "topic-1",
  question: "why?",
  answer: "because",
  status: "complete",
  createdAt: 1,
  updatedAt: 2,
  leafId: null,
  strategy: "native",
  truncated: false,
};

describe("BTW side-question wire contract", () => {
  it("accepts native and isolated execution without provider-specific strategy names", () => {
    for (const strategy of ["native", "isolated"]) {
      expect(SideTopicSchema.parse({ ...topic, strategy }).strategy).toBe(strategy);
    }
    expect(SideTopicSchema.safeParse({ ...topic, strategy: "codex" }).success).toBe(false);
  });

  it("keeps native record fields at the topic root with camelCase keys", () => {
    const parsed = SideTopicSchema.parse(topic);
    expect(parsed.createdAt).toBe(1);
    expect(parsed.leafId).toBeNull();
    expect(parsed.strategy).toBe("native");
  });

  it("accepts every native turn status including interrupted", () => {
    for (const status of ["running", "complete", "cancelled", "error", "interrupted"]) {
      expect(SideTopicSchema.parse({ ...topic, status }).status).toBe(status);
    }
    expect(SideTopicSchema.safeParse({ ...topic, status: "unknown" }).success).toBe(false);
  });

  it("derives the latest turn from follow-ups instead of the stale root status", () => {
    const withFollowUp = SideTopicSchema.parse({
      ...topic,
      followUps: [
        {
          question: "and?",
          answer: "",
          status: "running",
          createdAt: 3,
          updatedAt: 3,
        },
      ],
    });
    expect(latestSideTurn(withFollowUp).status).toBe("running");
    expect(canFollowUp(withFollowUp)).toBe(false);
    expect(canFollowUp(SideTopicSchema.parse(topic))).toBe(true);
  });

  it("bounds questions by UTF-8 bytes, not code units", () => {
    const atLimit = "a".repeat(SIDE_QUESTION_LIMITS.questionBytes);
    expect(SideAskRequestSchema.safeParse(ask(atLimit)).success).toBe(true);
    // Two-byte characters push the UTF-8 length past the bound while the code
    // unit count stays under it.
    const overByBytes = "a".repeat(SIDE_QUESTION_LIMITS.questionBytes - 1) + "é";
    expect(overByBytes.length).toBe(SIDE_QUESTION_LIMITS.questionBytes);
    expect(SideAskRequestSchema.safeParse(ask(overByBytes)).success).toBe(false);
  });

  it("rejects a projection larger than the topic bound", () => {
    const records = Array.from({ length: SIDE_QUESTION_LIMITS.topics + 1 }, (_, index) => ({
      ...topic,
      id: `topic-${index}`,
    }));
    expect(SideQuestionsStateSchema.safeParse({ records, omitted: 1 }).success).toBe(false);
    expect(
      SideQuestionsStateSchema.safeParse({ records: records.slice(0, 8), omitted: 1 }).success,
    ).toBe(true);
  });

  it("parses the frozen snapshot with its version and identity", () => {
    const snapshot = SideContextSnapshotSchema.parse({
      v: 1,
      agentId: "agent",
      provider: "omp",
      model: "qwen",
      capturedAt: 10,
      entries: [{ role: "user", text: "hi" }],
      truncated: true,
    });
    expect(snapshot.truncated).toBe(true);
    expect(SideContextSnapshotSchema.safeParse({ ...snapshot, entries: [] }).success).toBe(true);
  });

  it("rejects unknown request fields instead of silently dropping them", () => {
    // Rust request DTOs set deny_unknown_fields, so an extra key must fail
    // here rather than be stripped and sent as a different request.
    for (const request of [
      ask("q"),
      { type: "agent.side_question.cancel.request", requestId: "r1", agentId: "a" },
      { type: "agent.side_question.history.request", requestId: "r1", agentId: "a" },
    ]) {
      expect(SessionInboundMessageSchema.safeParse(request).success).toBe(true);
      expect(SessionInboundMessageSchema.safeParse({ ...request, extra: 1 }).success).toBe(false);
    }
  });

  it("travels through the inbound and outbound unions", () => {
    const request = ask("question");
    expect(SessionInboundMessageSchema.parse(request)).toMatchObject({
      type: "agent.side_question.ask.request",
    });
    const response = {
      type: "agent.side_question.ask.response",
      payload: { requestId: "r1", receipt: "accepted", recordId: "topic-1" },
    };
    expect(SessionOutboundMessageSchema.parse(response)).toMatchObject({
      payload: { receipt: "accepted" },
    });
    expect(
      WSOutboundMessageSchema.safeParse({
        type: "session",
        message: {
          type: "agent.side_question.history.response",
          payload: { requestId: "r2", records: [topic], omitted: 0 },
        },
      }).success,
    ).toBe(true);
  });

  it("surfaces providerUi.sideQuestions on the agent snapshot and stays optional", () => {
    const agent = {
      id: "agent-1",
      provider: "omp",
      cwd: "/workspace",
      model: null,
      createdAt: "2026-09-27T00:00:00Z",
      updatedAt: "2026-09-27T00:00:00Z",
      lastUserMessageAt: null,
      status: "idle",
      capabilities: {
        supportsStreaming: true,
        supportsSessionPersistence: true,
        supportsDynamicModes: false,
        supportsMcpServers: false,
        supportsReasoningStream: true,
        supportsToolInvocations: true,
      },
      currentModeId: null,
      availableModes: [],
      pendingPermissions: [],
      persistence: null,
      title: null,
    };
    // A host older than task09 never sends providerUi; that must stay valid.
    expect(AgentSnapshotPayloadSchema.parse(agent).providerUi).toBeUndefined();
    const parsed = AgentSnapshotPayloadSchema.parse({
      ...agent,
      providerUi: { sideQuestions: { records: [topic], omitted: 0, runningId: "topic-1" } },
    });
    expect(parsed.providerUi?.sideQuestions?.records[0].id).toBe("topic-1");
  });
});

function ask(question: string) {
  return {
    type: "agent.side_question.ask.request" as const,
    requestId: "r1",
    agentId: "agent-1",
    question,
  };
}

it("promotes full multi-turn history while keeping the live projection bounded", () => {
  const fullTopic = {
    ...topic,
    followUps: Array.from({ length: 12 }, (_, index) => ({
      question: `followup ${index}`,
      answer: `answer ${index}`,
      status: "complete",
      createdAt: index + 10,
      updatedAt: index + 11,
    })),
  };
  expect(SideTopicSchema.safeParse(fullTopic).success).toBe(false);
  const parsed = SideTransferResponseMessageSchema.parse({
    type: "agent.side_question.transfer.response",
    payload: {
      requestId: "transfer",
      topic: fullTopic,
      snapshot: {
        v: 1,
        agentId: "source",
        provider: "omp",
        model: "selected-model",
        capturedAt: 1,
        entries: [{ role: "assistant", text: "frozen context" }],
        truncated: false,
      },
    },
  });
  expect(parsed.payload.topic.followUps).toHaveLength(12);
  expect(parsed.payload.snapshot.entries[0].text).toBe("frozen context");
});
