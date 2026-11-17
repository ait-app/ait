// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface WriteRequest {
  content: string;
  expectedRevision?: string;
}

type WriteResult =
  | { status: "written"; size: number; modifiedAt: string; revision: string }
  | { status: "error"; error: string };

const mocks = vi.hoisted(() => {
  const writes: { request: WriteRequest; settle: (result: WriteResult) => void }[] = [];
  return {
    writes,
    client: {
      writeFile: vi.fn(
        (request: WriteRequest) =>
          new Promise<WriteResult>((resolve) => {
            writes.push({ request, settle: resolve });
          }),
      ),
      createFileEntry: vi.fn(),
    },
    liveFile: {
      snapshot: {
        observation: {
          status: "ready" as const,
          version: { status: "ready" as const, size: 5, modifiedAt: "t0", revision: "r0" },
        },
      },
      file: { bytes: new TextEncoder().encode("notes") },
      error: null,
      refresh: vi.fn(),
    },
  };
});

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/hooks/use-settings", () => ({
  useAppSettings: () => ({ settings: { experimentalFeatures: true } }),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHostRuntimeClient: () => mocks.client }));
vi.mock("@/file-pane/live-file/hook", () => ({ useLiveFile: () => mocks.liveFile }));
vi.mock("@/panels/pane-context", () => ({
  usePaneContext: () => ({
    serverId: "host-a",
    workspaceId: "workspace-a",
    target: { kind: "notebook" },
  }),
  usePaneFocus: () => ({ isInteractive: true }),
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

function type(text: string): void {
  fireEvent.change(screen.getByTestId("workspace-notebook-editor"), { target: { value: text } });
}

async function advance(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function settle(index: number, result: WriteResult): Promise<void> {
  await act(async () => {
    mocks.writes[index]!.settle(result);
    await Promise.resolve();
  });
}

describe("notebook autosave", () => {
  beforeEach(() => {
    vi.stubGlobal("React", React);
    vi.useFakeTimers();
    mocks.writes.length = 0;
    mocks.client.writeFile.mockClear();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("saves once typing pauses, not once per keystroke", async () => {
    render(<NotebookPanel />);

    type("notes a");
    await advance(300);
    type("notes ab");
    await advance(300);
    expect(mocks.writes).toHaveLength(0);

    await advance(200);
    expect(mocks.writes.map((write) => write.request.content)).toEqual(["notes ab"]);
  });

  it("saves text typed while an earlier save was still in flight", async () => {
    render(<NotebookPanel />);

    type("notes a");
    await advance(500);
    expect(mocks.writes).toHaveLength(1);

    // Typed during the first write: its timer fires before the write returns.
    type("notes ab");
    await advance(500);
    expect(mocks.writes).toHaveLength(1);

    await settle(0, { status: "written", size: 7, modifiedAt: "t1", revision: "r1" });
    await advance(500);

    expect(mocks.writes).toHaveLength(2);
    expect(mocks.writes[1]!.request).toMatchObject({
      content: "notes ab",
      expectedRevision: "r1",
    });

    await settle(1, { status: "written", size: 8, modifiedAt: "t2", revision: "r2" });
    expect(screen.getByText("panels.notebook.saved")).toBeTruthy();
  });

  it("tries again on the next edit after a failed save", async () => {
    render(<NotebookPanel />);

    type("notes a");
    await advance(500);
    await settle(0, { status: "error", error: "Transport closed" });
    expect(screen.getByText("panels.notebook.saveFailed")).toBeTruthy();

    type("notes ab");
    await advance(500);

    expect(mocks.writes).toHaveLength(2);
    expect(mocks.writes[1]!.request).toMatchObject({
      content: "notes ab",
      expectedRevision: "r0",
    });
  });
});
