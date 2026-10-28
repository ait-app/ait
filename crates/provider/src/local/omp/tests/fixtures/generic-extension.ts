import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  for (const name of ["compat-vendor-alpha", "compat-other-command"]) {
    pi.registerCommand(name, {
      description: `Generic compatibility fixture ${name}`,
      handler: async (_args, ctx) => {
        const selected = await ctx.ui.select(name, ["A, B", "C"]);
        ctx.ui.notify(`${name}: ${selected}`, "info");
        await pi.sendMessage({
          customType: name,
          content: `${name}: ${selected}`,
          display: true,
        });
      },
    });
  }
  pi.registerCommand("compat-notify-only", {
    description: "Notification without a custom message or model inference",
    handler: async (_args, ctx) => {
      ctx.ui.notify("OMP_NOTIFY_ONLY_OK", "info");
    },
  });
  for (const method of ["confirm", "input", "editor"] as const) {
    pi.registerCommand(`compat-dialog-${method}`, {
      description: `Generic ${method} dialog`,
      handler: async (_args, ctx) => {
        const title = `OMP ${method} fixture`;
        const answer =
          method === "confirm"
            ? await ctx.ui.confirm(title, "Continue this operation?")
            : method === "input"
              ? await ctx.ui.input(title, "Enter a response")
              : await ctx.ui.editor(title, "Initial plan");
        await pi.sendMessage({
          customType: "compat-dialog-result",
          content: `${method}: ${answer}`,
          display: true,
        });
      },
    });
  }
  pi.registerTool({
    name: "compat_dynamic_tool",
    label: "Generic fixture tool",
    description: "A tool registered by an isolated test extension",
    parameters: pi.zod.object({ text: pi.zod.string() }),
    async execute(_id, params) {
      return { content: [{ type: "text", text: params.text }], details: {} };
    },
  });
}
