/** @vitest-environment jsdom */
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  flushDraftPersistStorage,
  observeSharedDraftState,
  refreshSharedDraftState,
  useDraftStore,
} from "@/stores/draft-store";
import { EditingTextInput } from "@/components/ui/text-input/text-input.web";
import type { EditingTextInputHandle } from "@/components/ui/text-input";
import type { AttachmentMetadata, ComposerAttachment } from "@/attachments/types";
import { createWorkspaceFileAttachment } from "@/attachments/workspace-file";

const { asyncStorage } = vi.hoisted(() => ({
  asyncStorage: new Map<string, string>(),
}));

vi.hoisted(() => {
  (globalThis as unknown as { __DEV__: boolean }).__DEV__ = false;
});

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: {
    getItem: async (key: string) => asyncStorage.get(key) ?? null,
    setItem: async (key: string, value: string) => {
      asyncStorage.set(key, value);
    },
    removeItem: async (key: string) => {
      asyncStorage.delete(key);
    },
  },
}));

vi.mock("@/attachments/service", () => ({
  garbageCollectAttachments: async () => undefined,
}));

vi.mock("@/hooks/use-agent-form-state", () => ({
  useAgentFormState: () => ({
    selectedServerId: "host-1",
    selectedProvider: "codex",
    setProviderFromUser: () => undefined,
    selectedMode: "auto",
    setModeFromUser: () => undefined,
    selectedModel: "",
    setModelFromUser: () => undefined,
    selectedThinkingOptionId: "",
    setThinkingOptionFromUser: () => undefined,
    workingDir: "/repo",
    providerDefinitions: [{ id: "codex", label: "Codex", modes: [{ id: "auto", label: "Auto" }] }],
    providerDefinitionMap: new Map(),
    agentDefinition: undefined,
    modeOptions: [{ id: "auto", label: "Auto" }],
    availableModels: [
      {
        provider: "codex",
        id: "gpt-5.4",
        label: "gpt-5.4",
        isDefault: true,
        defaultThinkingOptionId: "high",
        thinkingOptions: [
          { id: "medium", label: "Medium" },
          { id: "high", label: "High", isDefault: true },
        ],
      },
    ],
    allProviderModels: new Map([
      [
        "codex",
        [
          {
            provider: "codex",
            id: "gpt-5.4",
            label: "gpt-5.4",
            isDefault: true,
            defaultThinkingOptionId: "high",
            thinkingOptions: [
              { id: "medium", label: "Medium" },
              { id: "high", label: "High", isDefault: true },
            ],
          },
        ],
      ],
    ]),
    modelSelectorProviders: [
      {
        id: "codex",
        label: "Codex",
        modelSelection: {
          kind: "models",
          rows: [
            {
              favoriteKey: "codex:gpt-5.4",
              provider: "codex",
              providerLabel: "Codex",
              modelId: "gpt-5.4",
              modelLabel: "gpt-5.4",
              isDefault: true,
            },
          ],
        },
      },
    ],
    isAllModelsLoading: false,
    isProviderModelsRefreshing: false,
    availableThinkingOptions: [
      { id: "medium", label: "Medium" },
      { id: "high", label: "High", isDefault: true },
    ],
    isModelLoading: false,
    modelError: null,
    refreshProviderModels: () => undefined,
    setProviderAndModelFromUser: () => undefined,
    workingDirIsEmpty: false,
    persistFormPreferences: async () => undefined,
  }),
}));

const mountedRoots = new Set<Root>();
function createTestRoot(container: HTMLElement): Root {
  const root = createRoot(container);
  mountedRoots.add(root);
  return root;
}

afterEach(async () => {
  await act(async () => {
    for (const root of mountedRoots) root.unmount();
    mountedRoots.clear();
    await flushDraftPersistStorage();
  });
});

let useAgentInputDraft: typeof import("./input-draft").useAgentInputDraft;
type DraftRecordForTest = ReturnType<typeof useDraftStore.getState>["drafts"][string];

beforeAll(async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    value: true,
    configurable: true,
    writable: true,
  });

  ({ useAgentInputDraft } = await import("./input-draft"));
});

describe("useAgentInputDraft live contract", () => {
  beforeEach(() => {
    asyncStorage.clear();
    document.body.innerHTML = "<div id='root'></div>";

    useDraftStore.setState({
      drafts: {},
      createModalDraft: null,
      attachmentFocusRequestByDraftKey: {},
    });
  });

  async function mountDraftEditor(draftKey: string) {
    let latest: ReturnType<typeof useAgentInputDraft>;
    function Editor() {
      const draft = useAgentInputDraft({ draftKey });
      latest = draft;
      const input = React.useRef<EditingTextInputHandle | null>(null);
      // MessageInput applies this same replacement signal to its DOM-owned editor.
      React.useEffect(() => {
        input.current?.replaceText(draft.textReplacement.text);
      }, [draft.textReplacement]);
      return (
        <EditingTextInput
          ref={input}
          initialValue={draft.textSource.getSnapshot()}
          onChangeText={draft.editText}
          multiline
        />
      );
    }
    const container = document.getElementById("root")!;
    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={new QueryClient()}>
          <Editor />
        </QueryClientProvider>,
      );
    });
    const textarea = container.querySelector("textarea")!;
    return { textarea, getLatest: () => latest! };
  }

  function saveRemoteText(draftKey: string, text: string, lifecycle = "active") {
    const saved = JSON.parse(asyncStorage.get("paseo-drafts")!);
    const previous = saved.state.drafts[draftKey];
    saved.state.drafts[draftKey] = {
      input: { text, attachments: [] },
      lifecycle,
      updatedAt: (previous?.updatedAt ?? 0) + 1,
      version: (previous?.version ?? 0) + 1,
    };
    asyncStorage.set("paseo-drafts", JSON.stringify(saved));
  }

  it("replaces mounted editor text after a shared save without turning local typing into replacements", async () => {
    const editor = await mountDraftEditor("draft:shared");
    await act(async () => {
      editor.getLatest().replaceText("local draft");
    });
    await flushDraftPersistStorage();
    const localReplacement = editor.getLatest().textReplacement;
    saveRemoteText("draft:shared", "text saved in another window");

    await act(async () => {
      expect(await refreshSharedDraftState()).toBe(true);
    });
    expect(editor.textarea.value).toBe("text saved in another window");
    expect(editor.getLatest().textReplacement).not.toBe(localReplacement);
    const sharedReplacement = editor.getLatest().textReplacement;
    fireEvent.change(editor.textarea, { target: { value: "continue editing" } });
    await act(async () => {
      await flushDraftPersistStorage();
    });
    expect(editor.getLatest().textSource.getSnapshot()).toBe("continue editing");
    expect(editor.getLatest().textReplacement).toBe(sharedReplacement);
  });

  it("keeps pending DOM text when shared refresh happens before its after-paint publication", async () => {
    const editor = await mountDraftEditor("draft:pending");
    await act(async () => {
      editor.getLatest().replaceText("original");
    });
    await flushDraftPersistStorage();
    const replacement = editor.getLatest().textReplacement;
    const frame = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(7);
    const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    try {
      fireEvent.change(editor.textarea, { target: { value: "unpublished local input" } });
      expect(editor.getLatest().textSource.getSnapshot()).toBe("original");
      saveRemoteText("draft:pending", "remote input");
      await act(async () => {
        expect(await refreshSharedDraftState()).toBe(false);
      });
      expect(editor.textarea.value).toBe("unpublished local input");
      expect(editor.getLatest().textSource.getSnapshot()).toBe("unpublished local input");
      expect(editor.getLatest().textReplacement).toBe(replacement);
      expect(
        JSON.parse(asyncStorage.get("paseo-drafts")!).state.drafts["draft:pending"].input.text,
      ).toBe("unpublished local input");
    } finally {
      frame.mockRestore();
      cancel.mockRestore();
    }
  });

  it("does not replace a mounted editor for another draft's shared save and clears it for a sent tombstone", async () => {
    const editor = await mountDraftEditor("draft:current");
    await act(async () => {
      editor.getLatest().replaceText("current input");
    });
    await flushDraftPersistStorage();
    const replacement = editor.getLatest().textReplacement;
    saveRemoteText("draft:unrelated", "other draft");
    await act(async () => {
      expect(await refreshSharedDraftState()).toBe(true);
    });
    expect(editor.getLatest().textReplacement).toBe(replacement);
    expect(editor.textarea.value).toBe("current input");
    saveRemoteText("draft:current", "", "sent");
    await act(async () => {
      expect(await refreshSharedDraftState()).toBe(true);
    });
    expect(editor.textarea.value).toBe("");
  });

  it("keeps an IME candidate during shared refresh and resumes synchronization after composition", async () => {
    const editor = await mountDraftEditor("draft:ime");
    await act(async () => {
      editor.getLatest().replaceText("original");
    });
    await flushDraftPersistStorage();
    const stop = observeSharedDraftState();
    try {
      fireEvent.compositionStart(editor.textarea);
      fireEvent.change(editor.textarea, { target: { value: "你好" } });
      expect(editor.getLatest().textSource.getSnapshot()).toBe("original");
      saveRemoteText("draft:ime", "另一窗口的草稿");
      await act(async () => {
        expect(await refreshSharedDraftState()).toBe(false);
      });
      expect(editor.textarea.value).toBe("你好");
      fireEvent.compositionEnd(editor.textarea);
      await act(async () => {
        await flushDraftPersistStorage();
      });
      expect(editor.getLatest().textSource.getSnapshot()).toBe("你好");
      saveRemoteText("draft:ime", "同步后的草稿");
      await act(async () => {
        expect(await refreshSharedDraftState()).toBe(true);
      });
      expect(editor.textarea.value).toBe("同步后的草稿");
    } finally {
      stop();
    }
  });

  it("rejects a shared snapshot when DOM input is staged during the asynchronous storage read", async () => {
    const editor = await mountDraftEditor("draft:read-pending");
    await act(async () => {
      editor.getLatest().replaceText("original");
    });
    await flushDraftPersistStorage();
    saveRemoteText("draft:read-pending", "remote snapshot");
    const storage = (await import("@react-native-async-storage/async-storage")).default;
    const getItem = storage.getItem;
    const started = Promise.withResolvers<void>();
    const resume = Promise.withResolvers<void>();
    let pause = true;
    const read = vi.spyOn(storage, "getItem").mockImplementation(async (name) => {
      const value = await getItem(name);
      if (pause) {
        pause = false;
        started.resolve();
        await resume.promise;
      }
      return value;
    });
    const frame = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(7);
    const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    try {
      await act(async () => {
        const refresh = refreshSharedDraftState();
        await started.promise;
        fireEvent.change(editor.textarea, { target: { value: "typed while reading" } });
        expect(editor.getLatest().textSource.getSnapshot()).toBe("original");
        resume.resolve();
        expect(await refresh).toBe(false);
        await flushDraftPersistStorage();
      });
      expect(editor.textarea.value).toBe("typed while reading");
      expect(editor.getLatest().textSource.getSnapshot()).toBe("typed while reading");
      expect(
        JSON.parse(asyncStorage.get("paseo-drafts")!).state.drafts["draft:read-pending"].input.text,
      ).toBe("typed while reading");
    } finally {
      resume.resolve();
      read.mockRestore();
      frame.mockRestore();
      cancel.mockRestore();
    }
  });

  it("checks for composition beginning during the asynchronous shared read", async () => {
    const editor = await mountDraftEditor("draft:ime-read");
    await act(async () => {
      editor.getLatest().replaceText("original");
    });
    await flushDraftPersistStorage();
    saveRemoteText("draft:ime-read", "remote");
    const stop = observeSharedDraftState();
    const storage = (await import("@react-native-async-storage/async-storage")).default;
    const getItem = storage.getItem;
    const read = vi.spyOn(storage, "getItem").mockImplementation(async (name) => {
      const result = await getItem(name);
      fireEvent.compositionStart(editor.textarea);
      fireEvent.change(editor.textarea, { target: { value: "未完成" } });
      return result;
    });
    try {
      await act(async () => {
        expect(await refreshSharedDraftState()).toBe(false);
      });
      expect(editor.textarea.value).toBe("未完成");
      expect(editor.getLatest().textSource.getSnapshot()).toBe("original");
    } finally {
      read.mockRestore();
      fireEvent.compositionEnd(editor.textarea);
      stop();
    }
  });

  it("releases the composition guard and browser listeners with the app lifecycle", () => {
    const removeDocumentListener = vi.spyOn(document, "removeEventListener");
    const removeWindowListener = vi.spyOn(window, "removeEventListener");
    try {
      const stop = observeSharedDraftState();
      stop();
      expect(removeDocumentListener).toHaveBeenCalledWith(
        "compositionstart",
        expect.any(Function),
        true,
      );
      expect(removeDocumentListener).toHaveBeenCalledWith(
        "compositionend",
        expect.any(Function),
        true,
      );
      expect(removeWindowListener).toHaveBeenCalledWith("storage", expect.any(Function));
      expect(removeWindowListener).toHaveBeenCalledWith("focus", expect.any(Function));
    } finally {
      removeDocumentListener.mockRestore();
      removeWindowListener.mockRestore();
    }
  });

  it("hydrates persisted text and attachments and returns draft-mode composer state for a caller-provided key", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const image: AttachmentMetadata = {
      id: "attachment-1",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/1",
      createdAt: 1,
      fileName: "image.png",
      byteSize: 128,
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    function Probe({ draftKey }: { draftKey: string }) {
      latest = useAgentInputDraft({
        draftKey,
        composer: {
          initialServerId: "host-1",
          isVisible: true,
          lockedWorkingDir: "/repo",
        },
      });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    let root: Root | null = createTestRoot(container);
    await act(async () => {
      root!.render(
        <QueryClientProvider client={queryClient}>
          <Probe draftKey="draft:setup" />
        </QueryClientProvider>,
      );
    });

    expect(getLatest().composerState?.agentControls.selectedProvider).toBe("codex");
    expect(getLatest().composerState?.commandDraftConfig).toEqual({
      provider: "codex",
      cwd: "/repo",
      modeId: "auto",
      model: "gpt-5.4",
      thinkingOptionId: "high",
    });

    const hydratedTextReplacement = getLatest().textReplacement;
    // The editor applies a retained caret to the hydrated text, so hydration announces itself.
    expect(hydratedTextReplacement.kind).toBe("hydrated");

    await act(async () => {
      getLatest().editText("hello world");
      getLatest().setAttachments([{ kind: "image", metadata: image }]);
    });

    expect(getLatest().textReplacement).toBe(hydratedTextReplacement);

    await act(async () => {
      getLatest().replaceText("replacement text");
    });

    expect(getLatest().textReplacement).not.toBe(hydratedTextReplacement);
    expect(getLatest().textReplacement.kind).toBe("replace");
    expect(getLatest().textReplacement.text).toBe("replacement text");

    await act(async () => {
      getLatest().editText("hello world");
    });

    await act(async () => {
      root!.unmount();
    });

    root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe draftKey="draft:setup" />
        </QueryClientProvider>,
      );
    });

    expect(getLatest().textSource.getSnapshot()).toBe("hello world");
    expect(getLatest().attachments).toEqual([{ kind: "image", metadata: image }]);
  });

  it("migrates legacy image drafts to image attachments on hydration", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const image: AttachmentMetadata = {
      id: "legacy-image",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/legacy-image",
      createdAt: 10,
      fileName: "legacy.png",
      byteSize: 512,
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    useDraftStore.setState({
      drafts: {
        "draft:legacy": {
          input: {
            text: "legacy text",
            images: [image],
          },
          lifecycle: "active",
          updatedAt: Date.now(),
          version: 1,
        } as unknown as DraftRecordForTest,
      },
      createModalDraft: null,
    });

    function Probe() {
      latest = useAgentInputDraft({ draftKey: "draft:legacy" });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>,
      );
    });

    expect(getLatest().textSource.getSnapshot()).toBe("legacy text");
    expect(getLatest().attachments).toEqual([{ kind: "image", metadata: image }]);
    expect(useDraftStore.getState().drafts["draft:legacy"]?.input).toEqual({
      text: "legacy text",
      attachments: [{ kind: "image", metadata: image }],
    });
  });

  it("hydrates drafts saved by old builds with cwd", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const githubIssue: ComposerAttachment = {
      kind: "github_issue",
      item: {
        kind: "issue",
        number: 42,
        title: "Unify attachments",
        url: "https://github.com/paseo/paseo/issues/42",
        state: "open",
        body: "body",
        labels: ["composer"],
      },
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    useDraftStore.setState({
      drafts: {
        "draft:new-shape": {
          input: {
            text: "new text",
            attachments: [githubIssue],
            cwd: "/persisted",
          },
          lifecycle: "active",
          updatedAt: Date.now(),
          version: 1,
        } as unknown as DraftRecordForTest,
      },
      createModalDraft: null,
    });

    function Probe() {
      latest = useAgentInputDraft({ draftKey: "draft:new-shape" });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>,
      );
    });

    expect(getLatest().textSource.getSnapshot()).toBe("new text");
    expect(getLatest().attachments).toEqual([githubIssue]);

    await act(async () => {
      root.unmount();
    });

    expect(useDraftStore.getState().drafts["draft:new-shape"]?.input).toEqual({
      text: "new text",
      attachments: [githubIssue],
      cwd: "/persisted",
    });
  });

  it("updates and persists attachments through setAttachments", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const image: AttachmentMetadata = {
      id: "next-image",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/next-image",
      createdAt: 11,
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    let renders = 0;
    function Probe() {
      renders += 1;
      latest = useAgentInputDraft({ draftKey: "draft:attachments" });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>,
      );
    });

    await act(async () => {
      getLatest().editText("with attachment");
      getLatest().setAttachments([{ kind: "image", metadata: image }]);
    });

    expect(getLatest().attachments).toEqual([{ kind: "image", metadata: image }]);
    const readPersistedInput = () => useDraftStore.getState().drafts["draft:attachments"]?.input;
    await act(async () => {
      // Web text publication occurs after paint; attachments save immediately.
      await expect.poll(readPersistedInput).toEqual({
        text: "with attachment",
        attachments: [{ kind: "image", metadata: image }],
      });
    });
    const settledRenders = renders;
    const attached = getLatest().attachments;
    const source = getLatest().textSource;
    let textNotifications = 0;
    const unsubscribe = source.subscribe(() => {
      textNotifications += 1;
    });
    await act(async () => {
      getLatest().editText("with attachment\n");
      await expect.poll(source.getSnapshot).toBe("with attachment\n");
    });
    expect(renders).toBe(settledRenders);
    expect(getLatest().attachments).toBe(attached);
    expect(getLatest().textSource).toBe(source);
    expect(textNotifications).toBe(1);
    unsubscribe();
    await act(async () => {
      root.unmount();
    });
  });

  it("attaches to an unmounted legacy draft without losing its input", async () => {
    const image: AttachmentMetadata = {
      id: "legacy-image",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/legacy-image",
      createdAt: 10,
    };
    useDraftStore.setState({
      drafts: {
        "draft:legacy-workspace-file": {
          input: { text: "legacy text", images: [image] },
          lifecycle: "active",
          updatedAt: Date.now(),
          version: 1,
        } as unknown as DraftRecordForTest,
      },
    });

    await useDraftStore.getState().attachWorkspaceFile({
      draftKey: "draft:legacy-workspace-file",
      attachment: createWorkspaceFileAttachment({ path: "src/app.ts" }),
    });

    expect(useDraftStore.getState().getDraftInput("draft:legacy-workspace-file")).toEqual({
      text: "legacy text",
      attachments: [
        { kind: "image", metadata: image },
        createWorkspaceFileAttachment({ path: "src/app.ts" }),
      ],
    });
  });

  it("clear resets text and attachments", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const image: AttachmentMetadata = {
      id: "clear-image",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/clear-image",
      createdAt: 12,
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    function Probe() {
      latest = useAgentInputDraft({ draftKey: "draft:clear" });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>,
      );
    });

    await act(async () => {
      getLatest().editText("queued message");
      getLatest().setAttachments([{ kind: "image", metadata: image }]);
    });

    await act(async () => {
      getLatest().clear("sent");
    });

    expect(getLatest().textSource.getSnapshot()).toBe("");
    expect(getLatest().attachments).toEqual([]);
    expect(useDraftStore.getState().drafts["draft:clear"]?.input).toEqual({
      text: "",
      attachments: [],
    });
  });

  it("clears drafts with sent and abandoned lifecycle tombstones", async () => {
    let latest: ReturnType<typeof useAgentInputDraft> | null = null;
    const sentImage: AttachmentMetadata = {
      id: "attachment-sent",
      mimeType: "image/png",
      storageType: "web-indexeddb",
      storageKey: "attachments/sent",
      createdAt: 2,
    };

    function getLatest(): ReturnType<typeof useAgentInputDraft> {
      if (!latest) {
        throw new Error("Expected hook result");
      }
      return latest;
    }

    function Probe() {
      latest = useAgentInputDraft({ draftKey: "draft:lifecycle" });
      return null;
    }

    const queryClient = new QueryClient();
    const container = document.getElementById("root");
    if (!container) {
      throw new Error("Missing root container");
    }

    const root = createTestRoot(container);
    await act(async () => {
      root.render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>,
      );
    });

    await act(async () => {
      getLatest().editText("queued message");
      getLatest().setAttachments([{ kind: "image", metadata: sentImage }]);
    });

    await act(async () => {
      getLatest().clear("sent");
    });

    expect(getLatest().textSource.getSnapshot()).toBe("");
    expect(getLatest().attachments).toEqual([]);
    expect(useDraftStore.getState().drafts["draft:lifecycle"]).toMatchObject({
      lifecycle: "sent",
      input: { text: "", attachments: [] },
    });

    await act(async () => {
      getLatest().editText("draft again");
    });

    await act(async () => {
      getLatest().clear("abandoned");
    });

    expect(useDraftStore.getState().drafts["draft:lifecycle"]).toMatchObject({
      lifecycle: "abandoned",
      input: { text: "", attachments: [] },
    });
  });
});
