// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface Notepad {
  workspaceId: string;
  revision: number;
  text: string;
}

interface WriteRequest {
  workspaceId: string;
  text: string;
  expected: number;
}

interface WriteResult {
  accepted: boolean;
  workspaceId: string;
  revision: number;
  error?: string;
}

const mocks = vi.hoisted(() => {
  const writes: {
    request: WriteRequest;
    settle: (result: WriteResult) => void;
    fail: (error: Error) => void;
  }[] = [];
  const state = {
    notepad: { workspaceId: "workspace-a", revision: 3, text: "notes" } as Notepad,
    readError: null as Error | null,
    legacyFile: null as string | null,
    experimental: true,
    supported: true,
    interactive: true,
  };
  return {
    writes,
    state,
    client: {
      getWorkspaceNotepad: vi.fn(async (_workspaceId: string) => {
        if (state.readError) throw state.readError;
        return { ...state.notepad };
      }),
      writeWorkspaceNotepad: vi.fn(
        (request: WriteRequest) =>
          new Promise<WriteResult>((resolve, reject) => {
            writes.push({ request, settle: resolve, fail: reject });
          }),
      ),
      readFile: vi.fn(async (_cwd: string, _path: string) => {
        if (state.legacyFile === null) throw new Error("File unavailable.");
        return { bytes: new TextEncoder().encode(state.legacyFile) };
      }),
      writeFile: vi.fn(),
      createFileEntry: vi.fn(),
    },
  };
});

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/hooks/use-settings", () => ({
  useAppSettings: () => ({ settings: { experimentalFeatures: mocks.state.experimental } }),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => mocks.client }));
vi.mock("@/runtime/host-features", () => ({
  useHostFeature: (_serverId: string, feature: string) =>
    feature === "workspaceNotepad" && mocks.state.supported,
}));
vi.mock("@/panels/pane-context", () => ({
  usePaneContext: () => ({
    serverId: "host-a",
    workspaceId: "workspace-a",
    target: { kind: "notebook" },
  }),
  usePaneFocus: () => ({ isInteractive: mocks.state.interactive }),
}));
vi.mock("@/panels/panel-instance-attributes", () => ({
  usePublishPanelInstanceAttributes: () => undefined,
}));
vi.mock("@/panels/panel-registry", () => ({
  definePanel: (_kind: string, implementation: { component: React.ComponentType }) =>
    implementation,
}));
vi.mock("@/panels/panel-presentations", () => ({ notebookPanelPresentation: {} }));
vi.mock("@/stores/session-store-hooks", () => ({ useWorkspaceDirectory: () => "/repo" }));

const { notebookPanelRegistration } = await import("./notebook-panel");
const NotebookPanel = (notebookPanelRegistration as unknown as { component: React.ComponentType })
  .component;

function editor(): HTMLTextAreaElement {
  return screen.getByTestId("workspace-notebook-editor") as HTMLTextAreaElement;
}

function type(text: string): void {
  fireEvent.change(editor(), { target: { value: text } });
}

async function advance(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

/** Renders the panel and lets its first load finish. */
async function open(): Promise<ReturnType<typeof render>> {
  const view = render(<NotebookPanel />);
  await advance(0);
  return view;
}

async function settle(index: number, result: Partial<WriteResult>): Promise<void> {
  await act(async () => {
    mocks.writes[index]!.settle({
      accepted: true,
      workspaceId: "workspace-a",
      revision: 0,
      ...result,
    });
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  vi.useFakeTimers();
  mocks.writes.length = 0;
  Object.assign(mocks.state, {
    notepad: { workspaceId: "workspace-a", revision: 3, text: "notes" },
    readError: null,
    legacyFile: null,
    experimental: true,
    supported: true,
    interactive: true,
  });
  for (const fn of Object.values(mocks.client)) fn.mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("notepad loading", () => {
  it("shows the workspace notepad and never touches the legacy file once it has text", async () => {
    await open();

    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledWith("workspace-a");
    expect(editor().value).toBe("notes");
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
    expect(mocks.client.readFile).not.toHaveBeenCalled();
    expect(mocks.client.writeFile).not.toHaveBeenCalled();
    expect(mocks.client.createFileEntry).not.toHaveBeenCalled();
  });

  it("asks an old host for nothing and says it must be updated", async () => {
    mocks.state.supported = false;
    await open();

    expect(screen.getByTestId("workspace-notebook-unsupported").textContent).toBe(
      "panels.notebook.unsupported",
    );
    await advance(10_000);
    expect(mocks.client.getWorkspaceNotepad).not.toHaveBeenCalled();
    expect(mocks.client.readFile).not.toHaveBeenCalled();
  });

  it("asks for nothing while experimental features are off", async () => {
    mocks.state.experimental = false;
    await open();

    expect(screen.getByTestId("workspace-notebook-disabled")).toBeTruthy();
    await advance(10_000);
    expect(mocks.client.getWorkspaceNotepad).not.toHaveBeenCalled();
  });

  it("is read-only for a guest the host refuses, and stops asking", async () => {
    mocks.state.readError = new Error("Forbidden requestType=workspace.notepad.get.request");
    await open();

    expect(screen.getByText("panels.notebook.readOnly")).toBeTruthy();
    type("mine");
    expect(editor().value).toBe("");
    await advance(10_000);
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);
    expect(mocks.writes).toHaveLength(0);
  });

  it("recovers on a later poll when the first load failed", async () => {
    mocks.state.readError = new Error("Transport closed");
    await open();
    expect(screen.getByText("panels.notebook.saveFailed")).toBeTruthy();

    mocks.state.readError = null;
    await advance(3_000);
    expect(editor().value).toBe("notes");
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
  });
});

describe("notepad polling", () => {
  it("polls about every three seconds and shows newer text from the host", async () => {
    await open();
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);

    mocks.state.notepad = { workspaceId: "workspace-a", revision: 4, text: "notes from an agent" };
    await advance(2_900);
    expect(editor().value).toBe("notes");
    await advance(100);
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(2);
    expect(editor().value).toBe("notes from an agent");

    // The next save builds on the polled revision.
    type("notes from an agent, and me");
    await advance(500);
    expect(mocks.writes[0]!.request).toMatchObject({ expected: 4 });
  });

  it("keeps unsaved text when the host has something newer", async () => {
    await open();
    type("notes of mine");
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 4, text: "notes from an agent" };
    await advance(500);
    expect(mocks.writes).toHaveLength(1);
    // A poll that answers while the save is in flight must not replace the text either.
    await advance(3_000);
    expect(editor().value).toBe("notes of mine");
    expect(mocks.writes[0]!.request).toMatchObject({ text: "notes of mine", expected: 3 });
  });

  it("does not poll in the background and refreshes at once when the pane becomes interactive", async () => {
    mocks.state.interactive = false;
    const view = await open();
    // The first load does not wait for focus.
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);
    expect(editor().value).toBe("notes");

    mocks.state.notepad = { workspaceId: "workspace-a", revision: 4, text: "newer" };
    await advance(10_000);
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);

    mocks.state.interactive = true;
    view.rerender(<NotebookPanel />);
    await advance(0);
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(2);
    expect(editor().value).toBe("newer");
  });

  it("ignores an older answer than the revision it already saved", async () => {
    await open();
    type("notes a");
    await advance(500);
    await settle(0, { revision: 9 });
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();

    // A read that started before the save answers with the revision the save replaced.
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 8, text: "stale" };
    await advance(3_000);
    expect(editor().value).toBe("notes a");
  });
});

describe("notepad autosave", () => {
  it("saves once typing pauses, not once per keystroke", async () => {
    await open();

    type("notes a");
    await advance(300);
    type("notes ab");
    await advance(300);
    expect(mocks.writes).toHaveLength(0);

    await advance(200);
    expect(mocks.writes.map((write) => write.request)).toEqual([
      { workspaceId: "workspace-a", text: "notes ab", expected: 3 },
    ]);
  });

  it("saves text typed while an earlier save was still in flight", async () => {
    await open();

    type("notes a");
    await advance(500);
    expect(mocks.writes).toHaveLength(1);

    // Typed during the first write: its timer fires before the write returns.
    type("notes ab");
    await advance(500);
    expect(mocks.writes).toHaveLength(1);

    await settle(0, { revision: 4 });
    await advance(500);

    expect(mocks.writes).toHaveLength(2);
    expect(mocks.writes[1]!.request).toMatchObject({ text: "notes ab", expected: 4 });

    await settle(1, { revision: 5 });
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
  });

  it("tries again on the next edit after a failed save", async () => {
    await open();

    type("notes a");
    await advance(500);
    await act(async () => {
      mocks.writes[0]!.fail(new Error("Transport closed"));
      await Promise.resolve();
    });
    expect(screen.getByText("panels.notebook.saveFailed")).toBeTruthy();

    type("notes ab");
    await advance(500);

    expect(mocks.writes).toHaveLength(2);
    expect(mocks.writes[1]!.request).toMatchObject({ text: "notes ab", expected: 3 });
  });

  it("clears the notepad when all text is deleted", async () => {
    await open();
    type("");
    await advance(500);
    expect(mocks.writes[0]!.request).toEqual({
      workspaceId: "workspace-a",
      text: "",
      expected: 3,
    });
  });

  it("reports a refused text as a failed save, not as a conflict", async () => {
    await open();
    type("x".repeat(10));
    await advance(500);
    // Same revision as expected: nobody else wrote, the host refused this text.
    await settle(0, { accepted: false, revision: 3, error: "The notepad is limited to 256 KiB." });

    expect(screen.getByText("panels.notebook.saveFailed")).toBeTruthy();
    expect(screen.queryByTestId("workspace-notebook-reload")).toBeNull();
  });

  it("becomes read-only when the host forbids the write", async () => {
    await open();
    type("notes a");
    await advance(500);
    await act(async () => {
      mocks.writes[0]!.fail(new Error("Forbidden code=Forbidden"));
      await Promise.resolve();
    });

    expect(screen.getByText("panels.notebook.readOnly")).toBeTruthy();
    type("notes ab");
    expect(editor().value).toBe("notes a");
  });
});

describe("notepad conflicts", () => {
  it("stops editing on a conflict and reloads the host's text on request", async () => {
    await open();
    type("notes of mine");
    await advance(500);
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 5, text: "notes from an agent" };
    await settle(0, { accepted: false, revision: 5, error: "The notepad changed." });

    expect(screen.getByText("panels.notebook.conflict")).toBeTruthy();
    type("more of mine");
    expect(editor().value).toBe("notes of mine");
    // Polling must not silently discard the unsaved text.
    await advance(3_000);
    expect(editor().value).toBe("notes of mine");
    expect(mocks.writes).toHaveLength(1);

    fireEvent.click(screen.getByTestId("workspace-notebook-reload"));
    await advance(0);
    expect(editor().value).toBe("notes from an agent");
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
    expect(screen.queryByTestId("workspace-notebook-reload")).toBeNull();

    type("notes from an agent, and me");
    await advance(500);
    expect(mocks.writes).toHaveLength(2);
    expect(mocks.writes[1]!.request).toMatchObject({
      text: "notes from an agent, and me",
      expected: 5,
    });
  });
});

describe("legacy notebook file import", () => {
  beforeEach(() => {
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 0, text: "" };
  });

  it("copies the old file into a never-written notepad and leaves the file alone", async () => {
    mocks.state.legacyFile = "﻿old notes\n";
    render(<NotebookPanel />);
    await advance(0);

    expect(mocks.client.readFile).toHaveBeenCalledWith("/repo", ".ait-notebook.md");
    expect(mocks.writes.map((write) => write.request)).toEqual([
      { workspaceId: "workspace-a", text: "old notes\n", expected: 0 },
    ]);
    // Still loading: nothing can be typed over the text about to arrive.
    type("typed too early");
    expect(editor().value).toBe("");

    await settle(0, { revision: 1 });
    expect(editor().value).toBe("old notes\n");
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
    expect(mocks.client.writeFile).not.toHaveBeenCalled();
    expect(mocks.client.createFileEntry).not.toHaveBeenCalled();

    type("old notes\nand new");
    await advance(500);
    expect(mocks.writes[1]!.request).toMatchObject({ expected: 1 });
    // Imported once: later polls of the still-open panel do not read the file again.
    await advance(6_000);
    expect(mocks.client.readFile).toHaveBeenCalledTimes(1);
  });

  it("starts empty when there is no old file, or it is blank", async () => {
    for (const legacyFile of [null, " \n"]) {
      mocks.state.legacyFile = legacyFile;
      const view = await open();
      expect(editor().value).toBe("");
      expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
      expect(mocks.writes).toHaveLength(0);
      view.unmount();
    }
  });

  it("starts empty when the import is refused", async () => {
    mocks.state.legacyFile = "old notes";
    render(<NotebookPanel />);
    await advance(0);
    await settle(0, { accepted: false, revision: 1, error: "The notepad changed." });

    expect(editor().value).toBe("");
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
    // Whoever wrote first shows up on the next poll.
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 1, text: "from an agent" };
    await advance(3_000);
    expect(editor().value).toBe("from an agent");
  });

  it("does not import over a notepad that was written and then cleared", async () => {
    mocks.state.notepad = { workspaceId: "workspace-a", revision: 2, text: "" };
    mocks.state.legacyFile = "old notes";
    await open();

    expect(mocks.client.readFile).not.toHaveBeenCalled();
    expect(editor().value).toBe("");
  });
});
