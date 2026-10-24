import { describe, expect, it, vi } from "vitest";
import type { AgentSnapshotPayload } from "@ait/protocol/messages";
import { createWorkspaceVoiceAgent } from "./new-workspace-voice";

function setup() {
  const agent = { id: "agent" } as AgentSnapshotPayload;
  return {
    config: { provider: "codex", cwd: "/repo", model: "selected-model", modeId: "auto" },
    ensureWorkspace: vi.fn(async () => ({ workspace: { id: "workspace" }, agent })),
    onCreated: vi.fn(),
    isCurrent: vi.fn(() => true),
    clearConsumedDraft: vi.fn(),
  };
}

describe("new workspace voice creation", () => {
  it("creates an empty Agent with the selected configuration and opens it before returning its ID", async () => {
    const input = setup();
    const id = await createWorkspaceVoiceAgent(input);
    expect(input.ensureWorkspace).toHaveBeenCalledExactlyOnceWith({
      cwd: "/repo",
      prompt: "",
      attachments: [],
      withInitialAgent: true,
      agent: { config: input.config },
    });
    expect(input.onCreated).toHaveBeenCalledExactlyOnceWith({ id: "workspace" }, { id: "agent" });
    expect(input.clearConsumedDraft).toHaveBeenCalledTimes(1);
    expect(id).toBe("agent");
  });

  it("keeps the draft on creation failure so the same screen can retry", async () => {
    const input = setup();
    input.ensureWorkspace.mockRejectedValueOnce(new Error("Host disconnected"));
    await expect(createWorkspaceVoiceAgent(input)).rejects.toThrow("Host disconnected");
    expect(input.clearConsumedDraft).not.toHaveBeenCalled();
    expect(input.onCreated).not.toHaveBeenCalled();
    await expect(createWorkspaceVoiceAgent(input)).resolves.toBe("agent");
  });

  it("does not navigate or request microphone startup after the user leaves", async () => {
    const input = setup();
    input.isCurrent.mockReturnValue(false);
    await expect(createWorkspaceVoiceAgent(input)).resolves.toBeNull();
    expect(input.onCreated).not.toHaveBeenCalled();
  });
});
