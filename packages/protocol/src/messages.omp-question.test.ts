import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

import { AgentSnapshotPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

test("OMP questions reach live clients and reconnect snapshots without a native message", () => {
  const { request } = JSON.parse(
    readFileSync(
      new URL("../../../crates/provider/tests/fixtures/omp_question.json", import.meta.url),
      "utf8",
    ),
  );
  const timestamp = "2026-10-28T02:00:00.000Z";
  const snapshot = AgentSnapshotPayloadSchema.parse({
    id: "agent",
    provider: "omp",
    cwd: "/workspace",
    model: null,
    createdAt: timestamp,
    updatedAt: timestamp,
    lastUserMessageAt: timestamp,
    status: "running",
    capabilities: {
      supportsStreaming: true,
      supportsSessionPersistence: true,
      supportsDynamicModes: false,
      supportsMcpServers: false,
      supportsReasoningStream: true,
      supportsToolInvocations: true,
    },
    currentModeId: "full",
    availableModes: [],
    pendingPermissions: [request],
    persistence: null,
    title: null,
  });
  expect(snapshot.pendingPermissions).toEqual([request]);
  const messages = [
    {
      type: "agent.stream",
      payload: {
        agentId: "agent",
        timestamp,
        event: { type: "permission_requested", provider: "omp", request },
      },
    },
    {
      type: "agent.get.response",
      payload: { requestId: "reconnect", agent: snapshot, error: null },
    },
  ];
  for (const message of messages) {
    const envelope = { type: "session", message };
    expect(WSOutboundMessageSchema.parse(envelope)).toEqual(envelope);
    expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
  }
});
