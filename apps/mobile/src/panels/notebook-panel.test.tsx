// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface Notepad {
  workspaceId: string;
  path: string;
  directory: string;
  revision: string;
  text: string;
}

interface FilePaneProps {
  serverId: string;
  workspaceRoot: string;
  location: { path: string };
  navigationRevision: number;
  initialMode?: "preview" | "source";
}

const mocks = vi.hoisted(() => {
  const state = {
    notepad: null as Notepad | null,
    readError: null as Error | null,
    experimental: true,
    supported: true,
    connected: true,
    workspaceId: "workspace-a",
    web: true,
  };
  return {
    state,
    filePanes: [] as FilePaneProps[],
    client: {
      getWorkspaceNotepad: vi.fn(async (workspaceId: string) => {
        if (state.readError) throw state.readError;
        return { ...state.notepad!, workspaceId };
      }),
      writeWorkspaceNotepad: vi.fn(),
      readFile: vi.fn(),
      writeFile: vi.fn(),
    },
  };
});

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("react-native-unistyles", () => ({
  StyleSheet: { create: (styles: unknown) => (typeof styles === "function" ? {} : styles) },
  withUnistyles: (component: unknown) => component,
}));
vi.mock("@/components/ui/button", () => ({
  Button: ({ children, onPress }: { children: React.ReactNode; onPress: () => void }) => (
    <button type="button" onClick={onPress}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/ui/icon-color", () => ({ mutedIconColorMapping: {} }));
vi.mock("@/components/ui/loading-spinner", () => ({ LoadingSpinner: () => null }));
vi.mock("@/constants/platform", () => ({
  get isWeb() {
    return mocks.state.web;
  },
}));
vi.mock("@/file-pane/pane", () => ({
  FilePane: (props: FilePaneProps) => {
    mocks.filePanes.push(props);
    return <div data-testid="workspace-file-pane">{props.location.path}</div>;
  },
}));
vi.mock("@/hooks/use-settings", () => ({
  useAppSettings: () => ({ settings: { experimentalFeatures: mocks.state.experimental } }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => (mocks.state.connected ? mocks.client : null),
}));
vi.mock("@/runtime/host-features", () => ({
  useHostFeature: (_serverId: string, feature: string) =>
    feature === "workspaceNotepad" && mocks.state.supported,
}));
vi.mock("@/panels/pane-context", () => ({
  usePaneContext: () => ({
    serverId: "host-a",
    workspaceId: mocks.state.workspaceId,
    target: { kind: "notebook" },
  }),
}));
vi.mock("@/panels/panel-registry", () => ({
  definePanel: (_kind: string, implementation: { component: React.ComponentType }) =>
    implementation,
}));
vi.mock("@/panels/panel-presentations", () => ({ notebookPanelPresentation: {} }));

const { notebookPanelRegistration } = await import("./notebook-panel");
const NotebookPanel = (notebookPanelRegistration as unknown as { component: React.ComponentType })
  .component;

/** Lets the pending notepad lookup settle. */
async function settle(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}

/** Renders the panel and lets its lookup finish. */
async function open(): Promise<ReturnType<typeof render>> {
  const view = render(<NotebookPanel />);
  await settle();
  return view;
}

function filePane(): FilePaneProps {
  expect(screen.getByTestId("workspace-file-pane")).toBeTruthy();
  return mocks.filePanes.at(-1)!;
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  mocks.filePanes.length = 0;
  Object.assign(mocks.state, {
    notepad: {
      workspaceId: "workspace-a",
      path: "/home/me/.ait/notepads/workspace-a/notepad.md",
      directory: "/home/me/.ait/notepads/workspace-a",
      revision: "r1",
      text: "notes",
    },
    readError: null,
    experimental: true,
    supported: true,
    connected: true,
    workspaceId: "workspace-a",
    web: true,
  });
  for (const fn of Object.values(mocks.client)) fn.mockClear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("workspace notepad panel", () => {
  it("opens the host's notepad file in the file pane, ready to type", async () => {
    await open();

    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledWith("workspace-a");
    expect(filePane()).toEqual({
      serverId: "host-a",
      workspaceRoot: "/home/me/.ait/notepads/workspace-a",
      location: { path: "notepad.md" },
      navigationRevision: 0,
      initialMode: "source",
    });
    // The file pane reads, watches and saves the file; the panel writes nothing itself.
    expect(mocks.client.writeWorkspaceNotepad).not.toHaveBeenCalled();
    expect(mocks.client.readFile).not.toHaveBeenCalled();
    expect(mocks.client.writeFile).not.toHaveBeenCalled();
  });

  it("shows the rendered notes where the file pane has no editor", async () => {
    mocks.state.web = false;
    await open();

    expect(filePane().initialMode).toBe("preview");
  });

  it("names the file of a host whose paths use backslashes", async () => {
    mocks.state.notepad = {
      ...mocks.state.notepad!,
      path: "C:\\Users\\me\\.ait\\notepads\\workspace-a\\notepad.md",
      directory: "C:\\Users\\me\\.ait\\notepads\\workspace-a",
    };
    await open();

    expect(filePane()).toMatchObject({
      workspaceRoot: "C:\\Users\\me\\.ait\\notepads\\workspace-a",
      location: { path: "notepad.md" },
    });
  });

  it("shows a spinner until the host answers", async () => {
    render(<NotebookPanel />);

    expect(screen.getByTestId("workspace-notebook-loading").textContent).toBe(
      "panels.notebook.loading",
    );
    await settle();
    expect(screen.queryByTestId("workspace-notebook-loading")).toBeNull();
  });

  it("asks an old host for nothing and says it must be updated", async () => {
    mocks.state.supported = false;
    await open();

    expect(screen.getByTestId("workspace-notebook-unsupported").textContent).toBe(
      "panels.notebook.unsupported",
    );
    expect(mocks.client.getWorkspaceNotepad).not.toHaveBeenCalled();
  });

  it("asks for nothing while experimental features are off", async () => {
    mocks.state.experimental = false;
    await open();

    expect(screen.getByTestId("workspace-notebook-disabled")).toBeTruthy();
    expect(mocks.client.getWorkspaceNotepad).not.toHaveBeenCalled();
  });

  it("offers a retry when the host cannot say where the notepad is", async () => {
    mocks.state.readError = new Error("Forbidden requestType=workspace.notepad.get.request");
    await open();

    expect(screen.getByTestId("workspace-notebook-error").textContent).toContain(
      "panels.notebook.loadFailed",
    );
    expect(screen.queryByTestId("workspace-file-pane")).toBeNull();
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);

    mocks.state.readError = null;
    fireEvent.click(screen.getByText("common.actions.retry"));
    await settle();
    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(2);
    expect(filePane().location).toEqual({ path: "notepad.md" });
  });

  it("keeps the file open across renders and while the host is away", async () => {
    const view = await open();
    const first = filePane();

    mocks.state.connected = false;
    view.rerender(<NotebookPanel />);
    await settle();
    mocks.state.connected = true;
    view.rerender(<NotebookPanel />);
    await settle();

    expect(mocks.client.getWorkspaceNotepad).toHaveBeenCalledTimes(1);
    // A new location object would make the file pane navigate again.
    expect(filePane().location).toBe(first.location);
  });

  it("looks the file up again for another workspace", async () => {
    const view = await open();
    mocks.state.workspaceId = "workspace-b";
    mocks.state.notepad = {
      ...mocks.state.notepad!,
      path: "/home/me/.ait/notepads/workspace-b/notepad.md",
      directory: "/home/me/.ait/notepads/workspace-b",
    };
    view.rerender(<NotebookPanel />);
    expect(screen.queryByTestId("workspace-file-pane")).toBeNull();
    await settle();

    expect(mocks.client.getWorkspaceNotepad).toHaveBeenLastCalledWith("workspace-b");
    expect(filePane().workspaceRoot).toBe("/home/me/.ait/notepads/workspace-b");
  });
});
