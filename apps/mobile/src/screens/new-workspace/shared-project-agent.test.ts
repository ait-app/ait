import { describe, expect, it, vi } from "vitest";
import type { AgentSnapshotPayload } from "@ait/protocol/messages";
import { createSharedProjectAgent } from "./shared-project-agent";

describe("shared project workspace creation", () => {
  it("creates the workspace before starting a guest Agent in its directory", async () => {
    const order: string[] = [];
    const createdAgent = { id: "agent-1" } as AgentSnapshotPayload;
    const createWorkspace = vi.fn(async () => {
      order.push("workspace");
      return {
        id: "workspace-1",
        workspaceDirectory: "/project/new-worktree",
      };
    });
    const createAgent = vi.fn(async () => {
      order.push("agent");
      return createdAgent;
    });

    const result = await createSharedProjectAgent({
      createWorkspace,
      createAgent,
      agent: {
        config: { provider: "codex", cwd: "/project" },
        initialPrompt: "hello",
        clientMessageId: "draft-1:initial-message",
      },
    });

    expect(order).toEqual(["workspace", "agent"]);
    expect(createAgent).toHaveBeenCalledWith({
      config: { provider: "codex", cwd: "/project/new-worktree" },
      initialPrompt: "hello",
      clientMessageId: "draft-1:initial-message",
      idempotencyKey: "draft-1:initial-message",
      workspaceId: "workspace-1",
    });
    expect(result).toBe(createdAgent);
  });

  it("does not start an Agent if workspace creation fails", async () => {
    const createAgent = vi.fn();
    await expect(
      createSharedProjectAgent({
        createWorkspace: async (): Promise<{
          id: string;
          workspaceDirectory: string;
        }> => {
          throw new Error("workspace failed");
        },
        createAgent,
        agent: { config: { provider: "codex", cwd: "/project" } },
      }),
    ).rejects.toThrow("workspace failed");
    expect(createAgent).not.toHaveBeenCalled();
  });
});
