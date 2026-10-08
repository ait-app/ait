/**
 * @vitest-environment jsdom
 */
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCreateFlowStore } from "@/stores/create-flow-store";
import type { UserMessageImageAttachment } from "@/types/stream";
import type { AgentAttachment } from "@ait/protocol/messages";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { useDraftAgentCreateFlow, type DraftCreateAttempt } from "./create-flow";
import { requestWorkspaceDraftAgent } from "./create-agent-request";

vi.mock("@/utils/encode-images", () => ({ encodeImages: vi.fn(async () => undefined) }));

describe("useDraftAgentCreateFlow", () => {
  beforeEach(() => {
    useCreateFlowStore.setState({ pendingByDraftId: {} });
  });

  it("renders a prepared new-workspace submission before continuing it", async () => {
    const image: UserMessageImageAttachment = {
      id: "image-1",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "image-key",
      createdAt: 123,
    };
    const attachment = {
      type: "review",
      cwd: "/repo",
      summary: "Review",
    } as unknown as AgentAttachment;
    const attempt: DraftCreateAttempt = {
      clientMessageId: "msg-prepared",
      text: "build this",
      timestamp: new Date("2026-05-25T00:00:00.000Z"),
      images: [image],
      attachments: [attachment],
    };
    const createRequest = vi.fn(
      async (ctx: {
        attempt: DraftCreateAttempt;
        text: string;
        images?: UserMessageImageAttachment[];
        attachments?: AgentAttachment[];
        cwd: string;
      }) => ({
        agentId: "agent-1",
        result: { id: "agent-1", ctx },
      }),
    );
    const onCreateSuccess = vi.fn();

    const { result } = renderHook(() =>
      useDraftAgentCreateFlow({
        draftId: "draft-1",
        getPendingServerId: () => "server-1",
        initialAttempt: attempt,
        buildDraftAgent: (currentAttempt) => ({ currentAttempt }),
        createRequest,
        onCreateSuccess,
      }),
    );

    expect(result.current.isSubmitting).toBe(true);
    expect(result.current.draftAgent).toEqual({ currentAttempt: attempt });
    expect(result.current.submittedStreamItems).toEqual([
      {
        kind: "user_message",
        id: "msg-prepared",
        clientMessageId: "msg-prepared",
        text: "build this",
        timestamp: attempt.timestamp,
        images: [image],
        attachments: [attachment],
      },
    ]);
    expect(createRequest).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.continueCreateFromAttempt({ attempt, cwd: "/repo" });
    });

    expect(createRequest).toHaveBeenCalledTimes(1);
    expect(createRequest).toHaveBeenCalledWith({
      attempt,
      text: "build this",
      images: [image],
      attachments: [attachment],
      cwd: "/repo",
    });
    expect(onCreateSuccess).toHaveBeenCalledTimes(1);
  });

  it("keeps the submitted preview when the pending provider selection disappears", () => {
    const attempt: DraftCreateAttempt = {
      clientMessageId: "msg-provider-handoff",
      text: "build this",
      timestamp: new Date("2026-05-25T00:00:00.000Z"),
    };
    const { result, rerender } = renderHook(
      ({ provider }: { provider: string | null }) =>
        useDraftAgentCreateFlow({
          draftId: "draft-handoff",
          getPendingServerId: () => "server-1",
          initialAttempt: attempt,
          buildDraftAgent: () => {
            if (!provider) throw new Error("Select a model");
            return { provider, model: "selected-model" };
          },
          createRequest: async () => ({ agentId: "agent-1", result: { id: "agent-1" } }),
          onCreateSuccess: () => undefined,
        }),
      { initialProps: { provider: "codex" } as { provider: string | null } },
    );
    expect(result.current.draftAgent).toEqual({ provider: "codex", model: "selected-model" });
    rerender({ provider: null });
    expect(result.current.isSubmitting).toBe(true);
    expect(result.current.draftAgent).toEqual({ provider: "codex", model: "selected-model" });
  });

  it("shows an invalid prepared selection as a form error instead of throwing in render", () => {
    const createRequest = vi.fn(async () => ({ agentId: "agent-1", result: { id: "agent-1" } }));
    const { result } = renderHook(() =>
      useDraftAgentCreateFlow({
        draftId: "draft-invalid",
        getPendingServerId: () => "server-1",
        initialAttempt: {
          clientMessageId: "msg-invalid",
          text: "build this",
          timestamp: new Date(0),
        },
        buildDraftAgent: () => {
          throw new Error("Select a model");
        },
        createRequest,
        onCreateSuccess: () => undefined,
      }),
    );
    expect(result.current.isSubmitting).toBe(false);
    expect(result.current.draftAgent).toBeNull();
    expect(result.current.formErrorMessage).toBe("Select a model");
    expect(createRequest).not.toHaveBeenCalled();
  });

  it("keeps a manual submission stable and lets a failed request retry with a new selection", async () => {
    let failRequest!: (error: Error) => void;
    const request = new Promise<never>((_resolve, reject) => {
      failRequest = reject;
    });
    const seenKeys = new Set<string>();
    const createAgent = vi.fn(async (options) => {
      if (seenKeys.has(options.idempotencyKey)) {
        throw new Error("Key was already used with different parameters");
      }
      seenKeys.add(options.idempotencyKey);
      if (seenKeys.size === 1) return await request;
      return { id: "agent-1" };
    });
    const client = { createAgent } as unknown as DaemonClient;
    const { result, rerender } = renderHook(
      ({ provider }: { provider: string | null }) =>
        useDraftAgentCreateFlow({
          draftId: "draft-manual",
          getPendingServerId: () => "server-1",
          buildDraftAgent: () => {
            if (!provider) throw new Error("Select a model");
            return { provider };
          },
          createRequest: async ({ attempt, text, cwd }) => {
            const agent = await requestWorkspaceDraftAgent(client, {
              workspaceId: "workspace-1",
              config: { provider: provider!, cwd },
              clientMessageId: attempt.clientMessageId,
              text,
            });
            return { agentId: agent.id, result: agent };
          },
          onCreateSuccess: () => undefined,
        }),
      { initialProps: { provider: "codex" } as { provider: string | null } },
    );
    let submission!: Promise<unknown>;
    const captureError = (error: unknown) => error;
    await act(async () => {
      submission = result.current
        .handleCreateFromInput({ text: "build this", attachments: [], cwd: "/repo" })
        .catch(captureError);
    });
    rerender({ provider: null });
    expect(result.current.draftAgent).toEqual({ provider: "codex" });
    await act(async () => {
      failRequest(new Error("Provider unavailable"));
      await submission;
    });
    expect(result.current.isSubmitting).toBe(false);
    expect(result.current.draftAgent).toBeNull();
    expect(result.current.formErrorMessage).toBe("Provider unavailable");
    rerender({ provider: "claude" });
    await act(async () => {
      await result.current.handleCreateFromInput({
        text: "try again",
        attachments: [],
        cwd: "/repo",
      });
    });
    expect(result.current.draftAgent).toEqual({ provider: "claude" });
    expect(result.current.formErrorMessage).toBe("");
    expect(createAgent).toHaveBeenCalledTimes(2);
    const [first, second] = createAgent.mock.calls.map(([options]) => options);
    expect(first.idempotencyKey).toBe(first.clientMessageId);
    expect(second.idempotencyKey).toBe(second.clientMessageId);
    expect(second.idempotencyKey).not.toBe(first.idempotencyKey);
    expect(second.initialPrompt).toBe("try again");
    expect(second.config.provider).toBe("claude");
  });

  it("keeps the creation and message identities when continuing the same attempt", async () => {
    const createAgent = vi.fn(async () => ({ id: "agent-1" }));
    const client = { createAgent } as unknown as DaemonClient;
    const attempt: DraftCreateAttempt = {
      clientMessageId: "draft-1:original-message",
      text: "build this",
      timestamp: new Date(0),
    };
    const { result } = renderHook(() =>
      useDraftAgentCreateFlow({
        draftId: "draft-1",
        getPendingServerId: () => "server-1",
        initialAttempt: attempt,
        buildDraftAgent: () => ({}),
        createRequest: async ({ attempt: current, text, cwd }) => {
          const agent = await requestWorkspaceDraftAgent(client, {
            workspaceId: "workspace-1",
            config: { provider: "opencode", cwd },
            clientMessageId: current.clientMessageId,
            text,
          });
          return { agentId: agent.id, result: agent };
        },
        onCreateSuccess: () => undefined,
      }),
    );
    await act(async () => {
      await result.current.continueCreateFromAttempt({ attempt, cwd: "/repo" });
      await result.current.continueCreateFromAttempt({ attempt, cwd: "/repo" });
    });
    expect(createAgent).toHaveBeenCalledTimes(2);
    expect(createAgent).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        idempotencyKey: attempt.clientMessageId,
        clientMessageId: attempt.clientMessageId,
      }),
    );
    expect(createAgent.mock.calls[0]).toEqual(createAgent.mock.calls[1]);
  });

  it("allows retrying an empty prompt when the draft still has context attachments", async () => {
    const attachment = {
      kind: "chat_history",
      id: "chat-history-1",
      attachment: {
        type: "text",
        mimeType: "text/plain",
        contextKind: "chat_history",
        title: "Chat history",
        text: "Previous conversation",
      },
      source: {
        serverId: "server-1",
        agentId: "agent-source",
      },
    } as const;
    const createRequest = vi.fn(async () => ({
      agentId: "agent-1",
      result: { id: "agent-1" },
    }));
    const onCreateSuccess = vi.fn();
    const validateBeforeSubmit = vi.fn(() => null);

    const { result } = renderHook(() =>
      useDraftAgentCreateFlow({
        draftId: "draft-1",
        getPendingServerId: () => "server-1",
        buildDraftAgent: (currentAttempt) => ({ currentAttempt }),
        createRequest,
        onCreateSuccess,
        validateBeforeSubmit,
      }),
    );

    await act(async () => {
      await result.current.handleCreateFromInput({
        text: "   ",
        attachments: [attachment],
        cwd: "/repo",
      });
    });

    expect(validateBeforeSubmit).toHaveBeenCalledWith({
      text: "",
      attachments: [attachment],
      cwd: "/repo",
    });
    expect(createRequest).toHaveBeenCalledWith({
      attempt: expect.objectContaining({
        text: "",
        attachments: [attachment.attachment],
      }),
      text: "",
      attachments: [attachment.attachment],
      cwd: "/repo",
    });
    expect(onCreateSuccess).toHaveBeenCalledTimes(1);
  });
});
