// Loaded by the user-installed OMP through its public extension API. No SDK imports at runtime.
export default function (pi) {
  let enabled = false;
  let previous = [];
  const names = ["vibe_spawn", "vibe_send", "vibe_wait", "vibe_kill", "vibe_list"];
  pi.registerCommand("ait-vibe-mode-v1", {
    description: "Ait external OMP director bridge version 1",
    handler: async (args, ctx) => {
      if (args === "on") {
        if (!enabled) previous = pi.getActiveTools();
        if (!names.every((name) => pi.getAllTools().some((tool) => tool.name === name))) {
          throw new Error("Ait Vibe host tools are not registered");
        }
        await pi.setActiveTools([
          ...previous.filter((name) => name === "read" || name === "todo"),
          ...names,
        ]);
        enabled = true;
        const route = (role) => {
          const model = ctx.models.resolve(role) ?? ctx.model;
          return model ? [model.provider, model.id] : null;
        };
        ctx.ui.notify(
          "AIT_VIBE_V1:" + JSON.stringify({ fast: route("@smol"), good: route("@task") }),
          "info",
        );
      } else if (args === "off") {
        if (enabled) await pi.setActiveTools(previous);
        enabled = false;
      } else {
        throw new Error("Expected on or off");
      }
    },
  });
  pi.registerCommand("ait-vibe-result-v1", {
    description: "Ait external OMP worker result delivery version 1",
    handler: async (args) => {
      if (!enabled) return;
      const result = JSON.parse(args);
      pi.sendMessage(
        { customType: "ait-vibe-result", content: JSON.stringify(result), display: true },
        { deliverAs: "followUp", triggerTurn: true },
      );
    },
  });
  pi.on("before_agent_start", (event) =>
    enabled
      ? {
          systemPrompt: [
            ...event.systemPrompt,
            "Vibe director mode: delegate work to persistent fast/good workers through vibe_spawn. " +
              "Use vibe_send for follow-ups on the same worker, vibe_list to inspect, vibe_wait for results, " +
              "and vibe_kill to stop workers. You may read files and maintain todos, but delegate edits, " +
              "shell commands and builds. Verify worker reports before giving the final answer. " +
              "Workers do not see this conversation; give each a self-contained brief.",
          ],
        }
      : undefined,
  );
}
