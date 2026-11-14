import type {
  EditorTarget,
  EditorTargetDescriptor,
  EditorTargetLaunchInput,
  EditorTargetRuntime,
} from "./target.js";
import { androidStudioTarget } from "./targets/android-studio.js";
import { antigravityTarget } from "./targets/antigravity.js";
import { cursorTarget } from "./targets/cursor.js";
import { explorerTarget, fileManagerTarget, finderTarget } from "./targets/file-manager.js";
import { kiroTarget } from "./targets/kiro.js";
import {
  aquaTarget,
  clionTarget,
  datagripTarget,
  dataspellTarget,
  golandTarget,
  intellijIdeaTarget,
  phpstormTarget,
  pycharmTarget,
  riderTarget,
  rubymineTarget,
  rustroverTarget,
  webstormTarget,
} from "./targets/jetbrains.js";
import { traeTarget } from "./targets/trae.js";
import { vscodiumTarget } from "./targets/vscodium.js";
import { vscodeInsidersTarget } from "./targets/vscode-insiders.js";
import { vscodeTarget } from "./targets/vscode.js";
import { zedTarget } from "./targets/zed.js";

export const EDITOR_TARGETS: readonly EditorTarget[] = [
  cursorTarget,
  traeTarget,
  kiroTarget,
  vscodeTarget,
  vscodeInsidersTarget,
  vscodiumTarget,
  zedTarget,
  antigravityTarget,
  androidStudioTarget,
  intellijIdeaTarget,
  aquaTarget,
  clionTarget,
  datagripTarget,
  dataspellTarget,
  golandTarget,
  phpstormTarget,
  pycharmTarget,
  riderTarget,
  rubymineTarget,
  rustroverTarget,
  webstormTarget,
  finderTarget,
  explorerTarget,
  fileManagerTarget,
];

export async function listAvailableEditorTargets(
  runtime: EditorTargetRuntime,
  targets: readonly EditorTarget[] = EDITOR_TARGETS,
): Promise<EditorTargetDescriptor[]> {
  const descriptors: EditorTargetDescriptor[] = [];
  for (const target of targets) {
    if (await target.isInstalled(runtime)) {
      descriptors.push(await target.describe(runtime));
    }
  }
  return descriptors;
}

export function getEditorTarget(
  id: string,
  targets: readonly EditorTarget[] = EDITOR_TARGETS,
): EditorTarget {
  const target = targets.find((candidate) => candidate.id === id);
  if (!target) throw new Error(`Unknown editor target: ${id}`);
  return target;
}

export async function openEditorTarget(
  input: EditorTargetLaunchInput & { editorId: string },
  runtime: EditorTargetRuntime,
  targets: readonly EditorTarget[] = EDITOR_TARGETS,
): Promise<void> {
  if (!runtime.isAbsolutePath(input.workspacePath)) {
    throw new Error("Editor target workspace path must be an absolute local path");
  }
  if (!runtime.pathExists(input.workspacePath)) {
    throw new Error(`Path does not exist: ${input.workspacePath}`);
  }
  if (!runtime.isDirectory(input.workspacePath)) {
    throw new Error(`Editor target workspace path must be a directory: ${input.workspacePath}`);
  }
  if (input.filePath) {
    if (!runtime.isAbsolutePath(input.filePath)) {
      throw new Error("Editor target file path must be an absolute local path");
    }
    if (!runtime.pathExists(input.filePath)) {
      throw new Error(`Path does not exist: ${input.filePath}`);
    }
  }

  const target = getEditorTarget(input.editorId, targets);
  if (!(await target.isInstalled(runtime))) {
    const descriptor = await target.describe(runtime);
    throw new Error(`Editor target unavailable: ${descriptor.label}`);
  }
  await target.launch(input, runtime);
}
