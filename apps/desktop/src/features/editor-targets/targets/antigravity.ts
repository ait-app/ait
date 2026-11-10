import { launchArgs, type EditorTarget } from "../target.js";

const COMMANDS = ["agy", "antigravity"] as const;

export const antigravityTarget: EditorTarget = {
  id: "antigravity",
  async describe(runtime) {
    return {
      id: this.id,
      label: "Antigravity",
      kind: "editor",
      icon: await runtime.loadIcon("antigravity.png"),
    };
  },
  async isInstalled(runtime) {
    return runtime.resolveCommand(COMMANDS) !== null;
  },
  async launch(input, runtime) {
    const command = runtime.resolveCommand(COMMANDS);
    if (!command) throw new Error("Antigravity is not installed");
    await runtime.spawnDetached({ command, args: launchArgs(input) });
  },
};
