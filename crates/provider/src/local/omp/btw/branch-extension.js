// Executed only inside the user-installed OMP. Uses its public extension/session APIs.
export function registerBtwBranch(pi) {
  pi.registerCommand("ait-btw-branch-v1", {
    description: "Promote a completed BTW answer through the public session API",
    handler: async (args, ctx) => {
      const input = JSON.parse(args);
      const manager = ctx.sessionManager;
      const originalFile = manager.getSessionFile();
      let changed = false;
      try {
        const jobs = ctx.getAsyncJobSnapshot?.();
        if (!ctx.isIdle() || ctx.hasPendingMessages() || jobs?.running?.length)
          throw new Error("The main conversation is busy");
        if (
          !originalFile ||
          manager.getSessionId() !== input.sessionId ||
          manager.getLeafId() !== input.leafId
        )
          throw new Error("The conversation changed since BTW started");
        if (!input.question?.trim() || !input.answer?.trim() || !input.leafId)
          throw new Error("The BTW answer is not ready");
        // Snapshot native entries, including compaction and model metadata. No copied prompt,
        // file surgery, SDK imports, or second process writing the journal.
        const entries = structuredClone(manager.getBranch());
        const labels = entries.flatMap((entry) => {
          const label = manager.getLabel(entry.id);
          return label ? [[entry.id, label]] : [];
        });
        const model = ctx.model;
        if (!model) throw new Error("No active model");
        const result = await ctx.newSession({
          parentSession: originalFile,
          setup: async (target) => {
            changed = true;
            for (const entry of entries) {
              if (entry.type === "label") continue;
              delete entry.activeLeafId;
              target.ingestReplicatedEntry(entry);
            }
            for (const [id, label] of labels) target.appendLabelChange(id, label);
            target.appendMessage({
              role: "user",
              content: [{ type: "text", text: input.question }],
              attribution: "user",
              timestamp: Date.now(),
            });
            target.appendMessage({
              role: "assistant",
              content: [{ type: "text", text: input.answer }],
              api: model.api,
              provider: model.provider,
              model: model.id,
              usage: {
                input: 0,
                output: 0,
                cacheRead: 0,
                cacheWrite: 0,
                totalTokens: 0,
                cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
              },
              stopReason: "stop",
              timestamp: Date.now(),
            });
            await target.flush();
          },
        });
        if (result.cancelled) throw new Error("Branch cancelled by OMP");
        // newSession's setup persists messages; switch reloads those messages into the live agent.
        const restored = await ctx.switchSession(manager.getSessionFile());
        if (restored.cancelled) throw new Error("OMP could not open the BTW branch");
        ctx.ui.notify(
          "AIT_BTW_BRANCH_V1:" + JSON.stringify({ requestId: input.requestId, success: true }),
          "info",
        );
      } catch (error) {
        if (changed) {
          const restored = await ctx.switchSession(originalFile);
          if (restored.cancelled)
            throw new Error(
              "BTW branch failed and OMP refused to restore the original conversation",
            );
        }
        ctx.ui.notify(
          "AIT_BTW_BRANCH_V1:" +
            JSON.stringify({
              requestId: input.requestId,
              success: false,
              error: error instanceof Error ? error.message : String(error),
            }),
          "info",
        );
      }
    },
  });
}
