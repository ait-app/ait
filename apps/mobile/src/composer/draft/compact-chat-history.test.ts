import { describe, expect, it, vi } from "vitest";
import type { AgentAttachment } from "@ait/protocol/messages";
import type { AgentContextCompactOptions } from "@ait/client/internal/daemon-client";
import { clearCompactedChatHistory, compactChatHistoryAttachments } from "./compact-chat-history";

const selection = {
  cwd: "/repo",
  provider: "claude",
  model: "sonnet",
  thinkingOptionId: "high",
};

function history(body: string): AgentAttachment[] {
  return [
    {
      type: "text",
      mimeType: "text/plain",
      contextKind: "chat_history",
      title: "Chat history",
      text: `<chat-history-summary>\nChat history from a previous Paseo agent.\n\n${body}</chat-history-summary>`,
    },
  ];
}

describe("compactChatHistoryAttachments", () => {
  it("uses the selected model for bounded segments and keeps recent source text", async () => {
    const compactAgentContext = vi.fn(
      async (_input: AgentContextCompactOptions) => "Earlier decisions and unresolved work.",
    );
    const attachments = history(`${"old decision\n".repeat(20_000)}LATEST REQUEST\n`);

    const result = await compactChatHistoryAttachments(
      { compactAgentContext },
      "draft-one",
      attachments,
      selection,
    );
    const text = result?.[0]?.type === "text" ? result[0].text : "";

    expect(compactAgentContext).toHaveBeenCalled();
    for (const [request] of compactAgentContext.mock.calls) {
      expect(request).toMatchObject({
        cwd: "/repo",
        provider: "claude",
        model: "sonnet",
        thinkingOptionId: "high",
      });
      expect(new TextEncoder().encode(request.context).length).toBeLessThanOrEqual(92 * 1024);
    }
    expect(text).toContain("Earlier conversation omitted during context compaction");
    expect(text).toContain("Earlier decisions and unresolved work.");
    expect(text).toContain("LATEST REQUEST");
    expect(new TextEncoder().encode(text).length).toBeLessThan(48 * 1024);
  });

  it("keeps only the recent source when compaction is unavailable", async () => {
    const compactAgentContext = vi.fn(async (_input: AgentContextCompactOptions) => {
      throw new Error("selected model unavailable");
    });
    const result = await compactChatHistoryAttachments(
      { compactAgentContext },
      "draft-fallback",
      history(`${"older text\n".repeat(15_000)}LATEST REQUEST\n`),
      selection,
    );
    const text = result?.[0]?.type === "text" ? result[0].text : "";

    expect(text).toContain("selected model could not compact");
    expect(text).toContain("LATEST REQUEST");
    expect(text).not.toContain("Earlier decisions");
  });

  it("does not request compression for small or non-history attachments", async () => {
    const compactAgentContext = vi.fn(async (_input: AgentContextCompactOptions) => "summary");
    const attachments = history("[User] recent question\n");

    const result = await compactChatHistoryAttachments(
      { compactAgentContext },
      "draft-small",
      attachments,
      selection,
    );

    expect(compactAgentContext).not.toHaveBeenCalled();
    expect(result).toBe(attachments);
    expect(
      await compactChatHistoryAttachments(
        { compactAgentContext },
        "draft-small",
        undefined,
        selection,
      ),
    ).toBeUndefined();
  });

  it("reuses a draft's result for the same model and clears it after send", async () => {
    const compactAgentContext = vi.fn(
      async (_input: AgentContextCompactOptions) => "Earlier decisions and unresolved work.",
    );
    const attachments = history(`${"old decision\n".repeat(20_000)}LATEST REQUEST\n`);

    const first = await compactChatHistoryAttachments(
      { compactAgentContext },
      "draft-cache",
      attachments,
      selection,
    );
    const firstCallCount = compactAgentContext.mock.calls.length;
    const cached = await compactChatHistoryAttachments(
      { compactAgentContext },
      "draft-cache",
      attachments,
      selection,
    );

    expect(cached).toBe(first);
    expect(compactAgentContext).toHaveBeenCalledTimes(firstCallCount);

    await compactChatHistoryAttachments({ compactAgentContext }, "draft-cache", attachments, {
      ...selection,
      model: "opus",
    });
    const changedModelCallCount = compactAgentContext.mock.calls.length;
    expect(changedModelCallCount).toBeGreaterThan(firstCallCount);

    clearCompactedChatHistory("draft-cache");
    const callsBeforeRecompact = compactAgentContext.mock.calls.length;
    await compactChatHistoryAttachments({ compactAgentContext }, "draft-cache", attachments, {
      ...selection,
      model: "opus",
    });
    expect(compactAgentContext.mock.calls.length - callsBeforeRecompact).toBe(firstCallCount);
  });
});
