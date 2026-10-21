import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { transformFileSync } from "@babel/core";
import { beforeEach, expect, test } from "vitest";
import * as draftKeys from "./draft-keys";
import type { CreateWorkspaceRequestOptions } from "@ait/client/internal/daemon-client";
import { useWorkspaceDraftSubmissionStore } from "./workspace-draft-submission-store";

beforeEach(() => {
  useWorkspaceDraftSubmissionStore.setState({
    pendingByDraftId: {},
    setupByDraftId: {},
    creationByDraftId: {},
    workspaceCreationByDraftId: {},
  });
});

// Exercise the emitted app code: plain TypeScript execution misses Expo's omit bug.
function loadProductionStore() {
  const filename = fileURLToPath(new URL("./workspace-draft-submission-store.ts", import.meta.url));
  const caller = { name: "metro", platform: "web", isDev: false, supportsStaticESM: false };
  const result = transformFileSync(filename, {
    cwd: fileURLToPath(new URL("../../", import.meta.url)),
    envName: "production",
    caller,
  });
  if (!result?.code) throw new Error("Expo produced no store code");
  const module = {
    exports: {} as { useWorkspaceDraftSubmissionStore: typeof useWorkspaceDraftSubmissionStore },
  };
  const require = createRequire(import.meta.url);
  new Function("require", "module", "exports", result.code)(
    (specifier: string) => (specifier === "@/stores/draft-keys" ? draftKeys : require(specifier)),
    module,
    module.exports,
  );
  return module.exports.useWorkspaceDraftSubmissionStore;
}

const storeVariants = Object.entries({
  TypeScript: () => useWorkspaceDraftSubmissionStore,
  "production Expo": loadProductionStore,
});

test.each(storeVariants)(
  "the %s store consumes a draft once and leaves other drafts pending",
  (_name, loadStore) => {
    const store = loadStore();
    const submission = {
      serverId: "server",
      workspaceId: "workspace",
      draftId: "draft-1",
      text: "hello",
      attachments: [],
      cwd: "/repo",
      provider: "codex",
      clientMessageId: "message",
      timestamp: 1,
    } as const;
    store.getState().setPending({ ...submission, attachments: [] });
    store.getState().setPending({ ...submission, attachments: [], draftId: "draft-2" });
    expect(store.getState().consumePending({ ...submission, workspaceId: "wrong" })).toBeNull();
    expect(store.getState().consumePending(submission)).toEqual(submission);
    expect(store.getState().consumePending(submission)).toBeNull();
    expect(Object.keys(store.getState().pendingByDraftId)).toEqual(["draft-2"]);
  },
);

function workspaceIntent(): CreateWorkspaceRequestOptions {
  return {
    source: {
      kind: "worktree",
      projectId: "project",
      cwd: "/repo",
      refName: "main",
      action: "branch-off",
    },
    agent: {
      config: {
        provider: "opencode",
        cwd: "/repo",
        model: "z-ai/glm-5.3",
        modeId: "build",
        thinkingOptionId: "high",
        featureValues: { permissionMode: "allow" },
      },
      initialPrompt: "Does ait support oh my pi?",
      clientMessageId: "draft-one:initial-message",
    },
    firstAgentContext: { prompt: "Does ait support oh my pi?", attachments: [] },
  };
}

test.each(storeVariants)(
  "the %s store retains the key and worktree slug of unchanged retries across lookups",
  (_name, loadStore) => {
    const store = loadStore();
    const input = { draftId: "draft-one", serverId: "gpu-desktop", intent: workspaceIntent() };
    const first = store.getState().prepareWorkspaceCreation(input);
    // A remounted screen rebuilds the request and obtains the same store-owned attempt.
    const retry = store
      .getState()
      .prepareWorkspaceCreation({ ...input, intent: workspaceIntent() });
    expect(retry).toBe(first);
    expect(first.idempotencyKey).not.toBe(input.draftId);
    expect(first.worktreeSlug).toBeTruthy();

    const reordered = JSON.parse(JSON.stringify(input.intent)) as CreateWorkspaceRequestOptions;
    reordered.agent!.config = {
      cwd: "/repo",
      provider: "opencode",
      model: "z-ai/glm-5.3",
      featureValues: { permissionMode: "allow", omitted: undefined },
      thinkingOptionId: "high",
      modeId: "build",
    };
    expect(store.getState().prepareWorkspaceCreation({ ...input, intent: reordered })).toBe(first);
  },
);

test.each([
  [
    "model",
    (intent) => {
      intent.agent!.config!.model = "another-model";
    },
  ],
  [
    "provider",
    (intent) => {
      intent.agent!.config!.provider = "codex";
    },
  ],
  [
    "mode",
    (intent) => {
      intent.agent!.config!.modeId = "plan";
    },
  ],
  [
    "thinking option",
    (intent) => {
      intent.agent!.config!.thinkingOptionId = "low";
    },
  ],
  [
    "feature values",
    (intent) => {
      intent.agent!.config!.featureValues = { permissionMode: "ask" };
    },
  ],
  [
    "prompt",
    (intent) => {
      intent.agent!.initialPrompt = "A revised question";
    },
  ],
  [
    "agent directory",
    (intent) => {
      intent.agent!.config!.cwd = "/repo/src";
    },
  ],
  [
    "images",
    (intent) => {
      intent.agent!.images = [{ data: "aW1hZ2U=", mimeType: "image/png" }];
    },
  ],
  [
    "attachments",
    (intent) => {
      intent.agent!.attachments = [{ type: "text", mimeType: "text/plain", text: "Context" }];
    },
  ],
  [
    "naming context",
    (intent) => {
      intent.firstAgentContext = { prompt: "A new title" };
    },
  ],
  [
    "branch",
    (intent) => {
      if (intent.source.kind === "worktree") intent.source.refName = "dev";
    },
  ],
  [
    "project",
    (intent) => {
      intent.source.projectId = "another-project";
    },
  ],
  [
    "source directory",
    (intent) => {
      if (intent.source.kind === "worktree") intent.source.cwd = "/another-repo";
    },
  ],
  [
    "isolation",
    (intent) => {
      intent.source = { kind: "directory", path: "/repo", projectId: "project" };
    },
  ],
  [
    "launch without agent",
    (intent) => {
      delete intent.agent;
    },
  ],
] satisfies [string, (intent: CreateWorkspaceRequestOptions) => void][])(
  "changing the %s after a failure creates a fresh attempt",
  (_field, change) => {
    const store = useWorkspaceDraftSubmissionStore;
    const input = { draftId: "draft-one", serverId: "gpu-desktop", intent: workspaceIntent() };
    const first = store.getState().prepareWorkspaceCreation(input);
    const changed = workspaceIntent();
    change(changed);
    const retry = store.getState().prepareWorkspaceCreation({ ...input, intent: changed });
    expect(retry.idempotencyKey).not.toBe(first.idempotencyKey);
    expect(store.getState().prepareWorkspaceCreation({ ...input, intent: changed })).toBe(retry);
  },
);

test("different hosts and drafts receive independent creation keys", () => {
  const store = useWorkspaceDraftSubmissionStore;
  const input = { draftId: "draft-one", serverId: "gpu-desktop", intent: workspaceIntent() };
  const first = store.getState().prepareWorkspaceCreation(input);
  const otherHost = store.getState().prepareWorkspaceCreation({ ...input, serverId: "laptop" });
  const otherDraft = store.getState().prepareWorkspaceCreation({ ...input, draftId: "draft-two" });
  expect(
    new Set([first.idempotencyKey, otherHost.idempotencyKey, otherDraft.idempotencyKey]).size,
  ).toBe(3);
});

test("consuming a draft releases only its own workspace creation attempt", () => {
  const store = useWorkspaceDraftSubmissionStore;
  const input = { draftId: "draft-one", serverId: "gpu-desktop", intent: workspaceIntent() };
  const first = store.getState().prepareWorkspaceCreation(input);
  const other = store.getState().prepareWorkspaceCreation({ ...input, draftId: "draft-two" });
  store.getState().clearDraftSetup({ draftId: " draft-one " });
  expect(store.getState().workspaceCreationByDraftId).toEqual({ "draft-two": other });
  expect(store.getState().prepareWorkspaceCreation(input).idempotencyKey).not.toBe(
    first.idempotencyKey,
  );
});

test.each(storeVariants)(
  "the %s store retains the running creation and identity through remount",
  (_name, loadStore) => {
    const store = loadStore();
    const result = new Promise<never>(() => {});
    const creation = { result, clientMessageId: "message" };
    const submission = {
      serverId: "server",
      workspaceId: "workspace",
      draftId: "draft-one",
      text: "hello",
      attachments: [],
      cwd: "/repo",
      provider: "codex",
      clientMessageId: "message",
      timestamp: 1,
      agentCreation: creation,
    };
    store.getState().setPending(submission);
    store.getState().consumePending(submission);
    expect(store.getState().pendingByDraftId[submission.draftId]).toBeUndefined();
    expect(store.getState().creationByDraftId[submission.draftId]).toBe(creation);
    store.getState().clearDraftSetup({ draftId: submission.draftId });
    expect(store.getState().creationByDraftId[submission.draftId]).toBeUndefined();
  },
);
