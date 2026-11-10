import { launchArgs, type EditorTarget, type EditorTargetRuntime } from "../target.js";

function commands(runtime: EditorTargetRuntime): string[] {
  const candidates = ["code-insiders"];
  if (runtime.platform === "darwin") {
    candidates.push(
      "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code",
    );
    if (runtime.env.HOME) {
      candidates.push(
        `${runtime.env.HOME}/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code`,
      );
    }
  }
  if (runtime.platform === "win32") {
    if (runtime.env.LOCALAPPDATA) {
      candidates.push(
        `${runtime.env.LOCALAPPDATA}/Programs/Microsoft VS Code Insiders/bin/code-insiders.cmd`,
      );
    }
    if (runtime.env.ProgramFiles) {
      candidates.push(
        `${runtime.env.ProgramFiles}/Microsoft VS Code Insiders/bin/code-insiders.cmd`,
      );
    }
  }
  return candidates;
}

export const vscodeInsidersTarget: EditorTarget = {
  id: "vscode-insiders",
  async describe() {
    return {
      id: this.id,
      label: "VS Code Insiders",
      kind: "editor",
      icon: { kind: "symbol", name: "terminal" },
    };
  },
  async isInstalled(runtime) {
    return (
      runtime.resolveCommand(commands(runtime)) !== null ||
      runtime.hasMacApplication("Visual Studio Code - Insiders")
    );
  },
  async launch(input, runtime) {
    const command = runtime.resolveCommand(commands(runtime));
    if (command) {
      await runtime.spawnDetached({ command, args: launchArgs(input) });
      return;
    }
    if (runtime.hasMacApplication("Visual Studio Code - Insiders")) {
      await runtime.openMacApplication({
        applicationName: "Visual Studio Code - Insiders",
        paths: input.filePath ? [input.workspacePath, input.filePath] : [input.workspacePath],
      });
      return;
    }
    throw new Error("VS Code Insiders is not installed");
  },
};
