import { launchArgs, type EditorTarget } from "../target.js";

const COMMANDS = ["trae"] as const;

export const traeTarget: EditorTarget = {
  id: "trae",
  async describe() {
    return {
      id: this.id,
      label: "Trae",
      kind: "editor",
      icon: { kind: "symbol", name: "terminal" },
    };
  },
  async isInstalled(runtime) {
    return runtime.resolveCommand(COMMANDS) !== null;
  },
  async launch(input, runtime) {
    const command = runtime.resolveCommand(COMMANDS);
    if (!command) throw new Error("Trae is not installed");
    await runtime.spawnDetached({ command, args: launchArgs(input) });
  },
};
