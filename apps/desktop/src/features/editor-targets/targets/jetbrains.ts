import type { EditorTarget } from "../target.js";

function jetBrainsTarget(input: {
  id: string;
  label: string;
  command: string;
  iconFileName?: string;
}): EditorTarget {
  const commands = [input.command, `${input.command}64`];
  return {
    id: input.id,
    async describe(runtime) {
      return {
        id: input.id,
        label: input.label,
        kind: "editor",
        icon: input.iconFileName
          ? await runtime.loadIcon(input.iconFileName)
          : { kind: "symbol", name: "terminal" },
      };
    },
    async isInstalled(runtime) {
      return runtime.resolveCommand(commands) !== null;
    },
    async launch(launchInput, runtime) {
      const command = runtime.resolveCommand(commands);
      if (!command) throw new Error(`${input.label} is not installed`);
      if (!launchInput.filePath) {
        await runtime.spawnDetached({ command, args: [launchInput.workspacePath] });
        return;
      }
      const args: string[] = [];
      if (launchInput.line) args.push("--line", String(launchInput.line));
      if (launchInput.column) args.push("--column", String(launchInput.column));
      args.push(launchInput.workspacePath, launchInput.filePath);
      await runtime.spawnDetached({ command, args });
    },
  };
}

export const intellijIdeaTarget = jetBrainsTarget({
  id: "intellij-idea",
  label: "IntelliJ IDEA",
  command: "idea",
});
export const aquaTarget = jetBrainsTarget({ id: "aqua", label: "Aqua", command: "aqua" });
export const clionTarget = jetBrainsTarget({ id: "clion", label: "CLion", command: "clion" });
export const datagripTarget = jetBrainsTarget({
  id: "datagrip",
  label: "DataGrip",
  command: "datagrip",
});
export const dataspellTarget = jetBrainsTarget({
  id: "dataspell",
  label: "DataSpell",
  command: "dataspell",
});
export const golandTarget = jetBrainsTarget({ id: "goland", label: "GoLand", command: "goland" });
export const phpstormTarget = jetBrainsTarget({
  id: "phpstorm",
  label: "PhpStorm",
  command: "phpstorm",
});
export const pycharmTarget = jetBrainsTarget({
  id: "pycharm",
  label: "PyCharm",
  command: "pycharm",
});
export const riderTarget = jetBrainsTarget({ id: "rider", label: "Rider", command: "rider" });
export const rubymineTarget = jetBrainsTarget({
  id: "rubymine",
  label: "RubyMine",
  command: "rubymine",
});
export const rustroverTarget = jetBrainsTarget({
  id: "rustrover",
  label: "RustRover",
  command: "rustrover",
});
export const webstormTarget = jetBrainsTarget({
  id: "webstorm",
  label: "WebStorm",
  command: "webstorm",
  iconFileName: "webstorm.png",
});
