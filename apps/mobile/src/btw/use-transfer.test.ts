import { beforeEach, expect, it, vi } from "vitest";
import { useTransferSideQuestion } from "./use-transfer";

const mocks = vi.hoisted(() => ({
  transfer: vi.fn(),
  attachments: vi.fn(),
  navigate: vi.fn(),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => ({ transferSideQuestion: mocks.transfer }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: {
    getState: () => ({
      sessions: {
        server: {
          agents: new Map([
            [
              "parent",
              {
                provider: "current-provider",
                model: "current-model",
                cwd: "/work",
                currentModeId: "default",
                features: [],
              },
            ],
          ]),
        },
      },
    }),
  },
}));
vi.mock("@/attachments/workspace-attachments-store", () => ({
  buildDraftWorkspaceAttachmentScopeKey: (id: string) => `scope:${id}`,
  useWorkspaceAttachmentsStore: {
    getState: () => ({ setWorkspaceAttachments: mocks.attachments }),
  },
}));
vi.mock("@/stores/draft-keys", () => ({ generateDraftId: () => "new-draft" }));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  navigateToWorkspace: mocks.navigate,
}));

beforeEach(() => {
  vi.resetAllMocks();
  mocks.transfer.mockResolvedValue({
    snapshot: {
      v: 1,
      agentId: "parent",
      provider: "frozen-provider",
      model: "frozen-model",
      capturedAt: 1,
      entries: [{ role: "user", text: "Frozen parent history" }],
      truncated: false,
    },
    topic: {
      id: "topic",
      question: "Question",
      answer: "Answer",
      status: "complete",
      createdAt: 1,
      updatedAt: 2,
      leafId: null,
      strategy: "isolated",
      truncated: false,
      followUps: [],
    },
  });
});

it("opens a normal editable draft with the frozen selection and visible BTW history, without sending", async () => {
  const transfer = useTransferSideQuestion({
    serverId: "server",
    agentId: "parent",
    workspaceId: "workspace",
  });
  await transfer("topic");
  expect(mocks.transfer).toHaveBeenCalledExactlyOnceWith({ agentId: "parent", recordId: "topic" });
  expect(mocks.attachments).toHaveBeenCalledWith({
    scopeKey: "scope:new-draft",
    attachments: [
      expect.objectContaining({
        kind: "chat_history",
        sideConversation: { topic: expect.objectContaining({ id: "topic" }) },
        attachment: expect.objectContaining({
          text: expect.stringContaining("Frozen parent history"),
        }),
      }),
    ],
  });
  expect(mocks.navigate).toHaveBeenCalledWith({
    serverId: "server",
    workspaceId: "workspace",
    target: {
      kind: "draft",
      draftId: "new-draft",
      setup: expect.objectContaining({
        provider: "frozen-provider",
        model: "frozen-model",
        cwd: "/work",
      }),
    },
  });
});

it("keeps the original window usable when the transfer read fails", async () => {
  mocks.transfer.mockRejectedValue(new Error("Connection lost"));
  await expect(
    useTransferSideQuestion({ serverId: "server", agentId: "parent", workspaceId: "workspace" })(
      "topic",
    ),
  ).rejects.toThrow("Connection lost");
  expect(mocks.navigate).not.toHaveBeenCalled();
  expect(mocks.attachments).not.toHaveBeenCalled();
});
