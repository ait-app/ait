export type EditorTargetKind = "editor" | "file-manager";

export type EditorTargetIcon =
  | { kind: "image"; dataUrl: string }
  | { kind: "symbol"; name: "folder" | "terminal" };

export interface EditorTargetDescriptor {
  id: string;
  label: string;
  kind: EditorTargetKind;
  icon: EditorTargetIcon;
}

export interface EditorTargetLaunchInput {
  workspacePath: string;
  filePath?: string;
  line?: number;
  column?: number;
}

export interface EditorTargetRuntime {
  readonly platform: NodeJS.Platform;
  readonly env: NodeJS.ProcessEnv;

  pathExists(path: string): boolean;
  isDirectory(path: string): boolean;
  realPath(path: string): string;
  isAbsolutePath(path: string): boolean;
  resolveCommand(commands: readonly string[]): string | null;
  spawnDetached(input: { command: string; args: readonly string[] }): Promise<void>;
  openPath(path: string): Promise<void>;
  revealPath(path: string): void;
  loadIcon(fileName: string): Promise<EditorTargetIcon>;
  hasMacApplication(applicationName: string): boolean;
  openMacApplication(input: { applicationName: string; paths: readonly string[] }): Promise<void>;
}

export interface EditorTarget {
  readonly id: string;

  describe(runtime: EditorTargetRuntime): Promise<EditorTargetDescriptor>;
  isInstalled(runtime: EditorTargetRuntime): Promise<boolean>;
  launch(input: EditorTargetLaunchInput, runtime: EditorTargetRuntime): Promise<void>;
}

export function location(input: EditorTargetLaunchInput): string {
  if (!input.line) return input.filePath!;
  return input.column
    ? `${input.filePath}:${input.line}:${input.column}`
    : `${input.filePath}:${input.line}`;
}

export function launchArgs(input: EditorTargetLaunchInput): string[] {
  if (!input.filePath) return [input.workspacePath];
  if (!input.line) return [input.workspacePath, input.filePath];
  return [input.workspacePath, "--goto", location(input)];
}
