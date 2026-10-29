import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { userEvent } from "vitest/browser";
import { EditorView } from "@codemirror/view";
import { getCM } from "@replit/codemirror-vim";
import { lightHighlightColors } from "@ait/highlight";
import { FileEditorModel, type FileEditorSession } from "./model";
import { FileEditorView } from "./view.web";
import type { EditorVisualTheme } from "./extensions.web";

vi.mock("../find/index.web", async () => ({
  FileFindModel: (await import("../find/model.web")).FileFindModel,
  FileFind: () => null,
}));

const theme: EditorVisualTheme = {
  colorScheme: "light",
  background: "#ffffff",
  foreground: "#111111",
  cursor: "#111111",
  foregroundMuted: "#666666",
  border: "#dddddd",
  selection: "#ccccff",
  monoFont: "monospace",
  codeFontSize: 14,
  syntax: lightHighlightColors,
};

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

function mountEditor(path: string) {
  const write = vi.fn<FileEditorSession["write"]>().mockImplementation(async ({ content }) => ({
    status: "written",
    size: content.length,
    modifiedAt: "2026-10-30T00:00:01.000Z",
    revision: "saved",
  }));
  const model = new FileEditorModel({
    file: {
      content: "before\n",
      hasBom: false,
      version: {
        status: "ready",
        cwd: "/workspace",
        path,
        size: 7,
        modifiedAt: "2026-10-30T00:00:00.000Z",
        revision: "initial",
      },
    },
    session: { write },
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  const onCursorChange = vi.fn();
  const onVimModeChange = vi.fn();
  function render(vimEnabled: boolean) {
    act(() => {
      root.render(
        <FileEditorView
          model={model}
          filename={path}
          location={{ path }}
          navigationRevision={0}
          vimEnabled={vimEnabled}
          theme={theme}
          onCursorChange={onCursorChange}
          onVimModeChange={onVimModeChange}
        />,
      );
    });
  }
  cleanups.push(() => {
    act(() => root.unmount());
    model.dispose();
    container.remove();
  });
  render(true);
  const content = container.querySelector<HTMLElement>(".cm-content");
  const mountedView = content && EditorView.findFromDOM(content);
  if (!mountedView) throw new Error("File editor did not mount");
  const view = mountedView;
  function edit(text: string) {
    act(() => {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
    });
    // Only the Vim command may trigger the write in these tests.
    model.suspendAutosave();
  }
  async function command(name: string) {
    await userEvent.click(view.contentDOM);
    await userEvent.keyboard(":");
    const input = container.querySelector<HTMLInputElement>(".cm-vim-panel input");
    if (!input) throw new Error("Vim command prompt did not open");
    await userEvent.type(input, name);
    await act(async () => {
      await userEvent.keyboard("{Enter}");
    });
  }
  return { view, model, write, edit, command, render };
}

test.each(["w", "write"])(":%s saves only its editor's dirty file", async (command) => {
  const first = mountEditor("first.txt");
  const second = mountEditor("second.txt");
  first.edit("first edit\n");
  second.edit("second edit\n");
  expect(first.write).not.toHaveBeenCalled();
  expect(second.write).not.toHaveBeenCalled();

  await first.command(command);
  expect(first.write).toHaveBeenCalledExactlyOnceWith({
    content: "first edit\n",
    expectedModifiedAt: "2026-10-30T00:00:00.000Z",
    expectedRevision: "initial",
  });
  expect(first.model.getSnapshot().status).toBe("clean");
  expect(second.write).not.toHaveBeenCalled();

  await second.command(command);
  expect(second.write).toHaveBeenCalledExactlyOnceWith({
    content: "second edit\n",
    expectedModifiedAt: "2026-10-30T00:00:00.000Z",
    expectedRevision: "initial",
  });
  expect(first.write).toHaveBeenCalledTimes(1);
});

test(":w still saves after toggling Vim off and on", async () => {
  const editor = mountEditor("toggle.txt");
  const previousCM = getCM(editor.view);
  editor.render(false);
  expect(previousCM?.save).toBeUndefined();
  editor.render(true);
  editor.edit("updated\n");

  await editor.command("w");
  expect(editor.write).toHaveBeenCalledOnce();
  expect(editor.model.getSnapshot().status).toBe("clean");
});
