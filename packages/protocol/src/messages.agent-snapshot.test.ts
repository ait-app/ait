import { describe, expect, it } from "vitest";
import { AgentSnapshotPayloadSchema, WSOutboundMessageSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";

const agent = {
  id: "agent-1",
  provider: "codex",
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

describe("Rust agent snapshots", () => {
  it.each([undefined, null, "Provider execution failed"])(
    "normalizes lastError=%s consistently in schema and generated validation",
    (lastError) => {
      const snapshot = { ...agent, ...(lastError === undefined ? {} : { lastError }) };
      const message = {
        type: "session",
        message: {
          type: "agent.get.response",
          payload: { requestId: "agent", agent: snapshot, error: null },
        },
      };
      const expected = WSOutboundMessageSchema.parse(message);
      expect(AgentSnapshotPayloadSchema.parse(snapshot).lastError).toBe(lastError ?? undefined);
      expect(validateWSOutboundMessage(message)).toEqual({ success: true, data: expected });
    },
  );

  it("accepts a populated Rust agent directory with no last error", () => {
    const message = {
      type: "session",
      message: {
        type: "agent.list.response",
        payload: {
          requestId: "agents",
          entries: [
            {
              agent: {
                ...agent,
                lastError: null,
                persistence: {
                  provider: "codex",
                  sessionId: "native-session",
                  nativeHandle: null,
                  metadata: null,
                },
                runtimeInfo: {
                  provider: "codex",
                  sessionId: "native-session",
                  extra: null,
                },
              },
              project: {
                projectKey: "directory:/workspace",
                projectName: "workspace",
                checkout: {
                  cwd: "/workspace",
                  isGit: false,
                  currentBranch: null,
                  remoteUrl: null,
                  isPaseoOwnedWorktree: false,
                  mainRepoRoot: null,
                },
              },
            },
          ],
          pageInfo: { nextCursor: null, prevCursor: null, hasMore: false },
        },
      },
    };
    expect(validateWSOutboundMessage(message)).toEqual({
      success: true,
      data: WSOutboundMessageSchema.parse(message),
    });
  });

  it.each([
    { nativeHandle: undefined, metadata: undefined, extra: undefined },
    { nativeHandle: null, metadata: null, extra: null },
    { nativeHandle: "resume-handle", metadata: { version: 1 }, extra: { mode: "auto" } },
  ])("normalizes live persistence and runtime fields: %j", (fields) => {
    const snapshot = {
      ...agent,
      persistence: {
        provider: "codex",
        sessionId: "native-session",
        nativeHandle: fields.nativeHandle,
        metadata: fields.metadata,
      },
      runtimeInfo: { provider: "codex", sessionId: "native-session", extra: fields.extra },
    };
    const message = {
      type: "session",
      message: {
        type: "agent.get.response",
        payload: { requestId: "agent", agent: snapshot, error: null },
      },
    };
    const parsed = AgentSnapshotPayloadSchema.parse(snapshot);
    expect(parsed.persistence?.nativeHandle).toEqual(fields.nativeHandle ?? undefined);
    expect(parsed.persistence?.metadata).toEqual(fields.metadata ?? undefined);
    expect(parsed.runtimeInfo?.extra).toEqual(fields.extra ?? undefined);
    expect(validateWSOutboundMessage(message)).toEqual({
      success: true,
      data: WSOutboundMessageSchema.parse(message),
    });
  });

  it.each([
    { persistence: { provider: "codex", sessionId: "native-session", nativeHandle: 42 } },
    { persistence: { provider: "codex", sessionId: "native-session", metadata: [] } },
    { runtimeInfo: { provider: "codex", sessionId: "native-session", extra: "invalid" } },
  ])("rejects malformed live fields: %j", (fields) => {
    const message = {
      type: "session",
      message: {
        type: "agent.get.response",
        payload: { requestId: "agent", agent: { ...agent, ...fields }, error: null },
      },
    };
    expect(WSOutboundMessageSchema.safeParse(message).success).toBe(false);
    expect(validateWSOutboundMessage(message).success).toBe(false);
  });

  it("continues rejecting malformed non-string errors", () => {
    const message = {
      type: "session",
      message: {
        type: "agent.get.response",
        payload: { requestId: "agent", agent: { ...agent, lastError: 42 }, error: null },
      },
    };
    expect(WSOutboundMessageSchema.safeParse(message).success).toBe(false);
    expect(validateWSOutboundMessage(message).success).toBe(false);
  });
});
