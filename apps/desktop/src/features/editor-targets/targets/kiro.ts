import { location, type EditorTarget, type EditorTargetLaunchInput } from "../target.js";

const COMMANDS = ["kiro"] as const;

function launchArgs(input: EditorTargetLaunchInput): string[] {
  if (!input.filePath) return ["ide", input.workspacePath];
  if (!input.line) return ["ide", input.workspacePath, input.filePath];
  return ["ide", input.workspacePath, "--goto", location(input)];
}

export const kiroTarget: EditorTarget = {
  id: "kiro",
  async describe() {
    return {
      id: this.id,
      label: "Kiro",
      kind: "editor",
      icon: { kind: "symbol", name: "terminal" },
    };
  },
  async isInstalled(runtime) {
    return runtime.resolveCommand(COMMANDS) !== null;
  },
  async launch(input, runtime) {
    const command = runtime.resolveCommand(COMMANDS);
    if (!command) throw new Error("Kiro is not installed");
    await runtime.spawnDetached({ command, args: launchArgs(input) });
  },
};
