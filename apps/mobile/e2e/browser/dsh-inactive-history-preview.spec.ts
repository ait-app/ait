import { expect, test } from "../support/fixtures";
import { existsSync, readFileSync, mkdtempSync, writeFileSync } from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type {
  DaemonClient,
  FetchProviderSessionPreviewEntry,
} from "@ait/client/internal/daemon-client";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { seedWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";
import { buildHostWorkspaceRoute } from "@/utils/host-routes";
import { gotoAppShell } from "../support/helpers/app";

// Read-only DSH preview through the real browser app, a real daemon and a loopback native
// Host fixture subprocess. The fixture records every native request so the test can prove
// browsing inactive history never creates, follows, prompts or selects a model.

const fixtureDirectory = mkdtempSync(path.join(tmpdir(), "ait-dsh-preview-"));
const auditPath = path.join(fixtureDirectory, "native-audit.jsonl");
const statePath = path.join(fixtureDirectory, "native-turn-count");
writeFileSync(statePath, "30");
const launcher = path.resolve("e2e/fixtures/fake-dsh-launcher.sh");

test.use({
  e2eDaemonConfig: {
    relay: { enabled: false },
    mcp: { enabled: true, injectIntoAgents: true },
    browserTools: { enabled: false },
    providers: {
      codex: { enabled: false },
      claude: { enabled: false },
      omp: { enabled: false },
      opencode: { enabled: false },
      pi: { enabled: false },
      antigravity: { enabled: false },
      "minimax-code": { enabled: false },
    },
    metadataGeneration: { providers: [] },
    autoArchiveAfterMerge: false,
    enableTerminalAgentHooks: false,
    appendSystemPrompt: "",
    cors: { allowedOrigins: [] },
    trustedProxies: ["loopback"],
    git: { maxProcessesPerSecond: 64, maxProcessConcurrency: 8 },
    pluginsEnabled: false,
    plugins: {},
  },
  e2eDaemonEnvironment: {
    AIT_SERVER_DEEPSEEK_HARNESS_BIN: launcher,
    DSH_E2E_AUDIT: auditPath,
    DSH_E2E_STATE: statePath,
  },
});

test.afterAll(async () => {
  await rm(fixtureDirectory, { recursive: true, force: true });
});

type PreviewClient = Pick<
  DaemonClient,
  | "connect"
  | "close"
  | "getProvidersSnapshot"
  | "fetchRecentProviderSessions"
  | "fetchProviderSessionPreview"
>;

function messageText({ item }: FetchProviderSessionPreviewEntry): string {
  if (item.type === "user_message" || item.type === "assistant_message") return item.text;
  throw new Error(`Expected a fixture message, received ${item.type}`);
}

interface NativeAuditEntry {
  kind: string;
  method: string;
  args?: {
    request?: {
      throughSeq?: number;
      address?: { sessionId?: string };
    };
  };
}

function readAudit(): NativeAuditEntry[] {
  return readFileSync(auditPath, "utf8")
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as NativeAuditEntry);
}

const WRITER_METHODS = [
  "session/follow",
  "session/create",
  "session/prompt",
  "session/cancel",
  "session/selectModel",
  "commands/execute",
  "session/new",
  "session/load",
];

function expectNoWriterRequests(scope: string) {
  const offenders = readAudit().filter((entry) => WRITER_METHODS.includes(entry.method));
  expect(offenders, `${scope}: native writer requests while browsing`).toEqual([]);
}

test("previews inactive DSH history read-only with fixed-cut pagination", async ({
  page,
}, testInfo) => {
  test.setTimeout(180_000);
  const workspace = await seedWorkspace({ repoPrefix: "ait-dsh-preview-", git: false });
  // The fixture Host lists its session under the cwd the daemon launches it with.
  const client = await connectDaemonClient<PreviewClient>({ clientIdPrefix: "dsh-preview" });
  try {
    // Provider discovery is asynchronous; wait until the catalog settles and advertises
    // the read-only preview capability instead of racing the first loading snapshot.
    await expect
      .poll(
        async () => {
          const snapshot = await client.getProvidersSnapshot({ cwd: workspace.repoPath });
          return snapshot.entries.find((entry) => entry.provider === "deepseek-harness")
            ?.supportsSessionPreview;
        },
        { timeout: 30_000, intervals: [500] },
      )
      .toBe(true);

    const recent = await client.fetchRecentProviderSessions({
      cwd: workspace.repoPath,
      providers: ["deepseek-harness"],
      limit: 20,
    });
    const entry = recent.entries.find((item) => item.providerHandleId === "e2e-native-session");
    expect(entry).toBeTruthy();
    if (!entry) throw new Error("DSH session was not listed");
    expect(entry.cwd).toBe(workspace.repoPath);
    const runningEntry = recent.entries.find(
      (item) => item.providerHandleId === "e2e-running-session",
    );
    if (!runningEntry) throw new Error("Running DSH control session was not listed");
    const runningBefore = await client.fetchProviderSessionPreview({
      ...runningEntry,
      limit: 100,
    });
    expect(runningBefore.active).toBe(true);

    // Typed SDK reads: newest page, then the older page on the same fixed cut.
    const first = await client.fetchProviderSessionPreview({
      providerId: entry.providerId,
      providerHandleId: entry.providerHandleId,
      cwd: entry.cwd,
      limit: 4,
    });
    expect(first.entries.map(messageText)).toEqual([
      "answer 29",
      "question 29",
      "answer 28",
      "question 28",
    ]);
    expect(first.hasOlder).toBe(true);
    expect(first.olderCursor).toBeTruthy();
    expect(first.active).toBe(false);
    // A later Host sees more persisted history, but this cursor must retain its original cut.
    writeFileSync(statePath, "33");
    const allEntries = [...first.entries];
    let cursor = first.olderCursor;
    while (cursor && allEntries.length < 60) {
      const older = await client.fetchProviderSessionPreview({
        providerId: entry.providerId,
        providerHandleId: entry.providerHandleId,
        cwd: entry.cwd,
        cursor,
        limit: 100,
      });
      expect(older.entries.length).toBeGreaterThan(0);
      expect(older.olderCursor).not.toBe(cursor);
      expect(older.hasOlder).toBe(older.olderCursor !== null);
      allEntries.push(...older.entries);
      cursor = older.olderCursor;
    }
    expect(cursor).toBeNull();
    expect(allEntries.map(messageText)).toEqual(
      Array.from({ length: 30 }, (_, index) => [
        `answer ${29 - index}`,
        `question ${29 - index}`,
      ]).flat(),
    );
    expect(new Set(allEntries.map((item) => item.key)).size).toBe(60);
    expectNoWriterRequests("typed SDK preview reads");
    const pages = readAudit().filter(
      (item) =>
        item.method === "session/page" &&
        item.args?.request?.address?.sessionId === entry.providerHandleId,
    );
    expect(pages.length).toBeGreaterThan(1);
    const cuts = new Set(pages.map((item) => item.args?.request?.throughSeq));
    expect(cuts.size).toBe(1);

    // Forged cross-session cursors are rejected before any native read.
    const beforeForged = readAudit().length;
    await expect(
      client.fetchProviderSessionPreview({
        providerId: entry.providerId,
        providerHandleId: "another-session",
        cwd: entry.cwd,
        cursor: first.olderCursor ?? undefined,
      }),
    ).rejects.toThrow();
    expect(readAudit().length).toBe(beforeForged);
    await expect(
      client.fetchProviderSessionPreview({
        providerId: entry.providerId,
        providerHandleId: entry.providerHandleId,
        cwd: fixtureDirectory,
        cursor: first.olderCursor ?? undefined,
      }),
    ).rejects.toThrow();
    expect(readAudit().length).toBe(beforeForged);

    // Browser journey: import sheet -> read-only preview -> load older.
    await gotoAppShell(page);
    await page.goto(buildHostWorkspaceRoute(getServerId(), workspace.workspaceId));
    await page.getByTestId("sidebar-import-session").click();
    const row = page.getByTestId(
      `import-session-session-deepseek-harness-${entry.providerHandleId}`,
    );
    await expect(row).toBeVisible({ timeout: 30_000 });
    await page
      .getByTestId(`import-session-preview-deepseek-harness-${entry.providerHandleId}`)
      .click();
    const sheet = page.getByTestId("session-preview-sheet");
    await expect(sheet).toBeVisible({ timeout: 30_000 });
    await expect(page.getByText("answer 32", { exact: true })).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId("session-preview-active-notice")).toHaveCount(0);
    const messages = sheet.getByText(/^(question|answer) \d+$/);
    await expect(messages).toHaveCount(50);
    while ((await messages.count()) < 66) {
      const previousCount = await messages.count();
      await page.getByTestId("session-preview-load-older").click();
      await expect.poll(() => messages.count()).toBeGreaterThan(previousCount);
    }
    await expect(messages).toHaveText(
      Array.from({ length: 33 }, (_, turn) => [`question ${turn}`, `answer ${turn}`]).flat(),
    );
    await expect(page.getByTestId("session-preview-load-older")).toHaveCount(0);
    expectNoWriterRequests("browser preview browsing");

    // Closing the preview returns to the import sheet without importing anything.
    await page.keyboard.press("Escape");
    await expect(row).toBeVisible({ timeout: 30_000 });
    expectNoWriterRequests("after closing preview");
    const runningAfter = await client.fetchProviderSessionPreview({
      ...runningEntry,
      limit: 100,
    });
    // Correlation IDs identify different RPCs; every native fact must remain identical.
    expect(runningAfter).toEqual({ ...runningBefore, requestId: runningAfter.requestId });
    expectNoWriterRequests("running control remains unchanged");
  } finally {
    try {
      await testInfo.attach("preview-before-cleanup", {
        body: await page.screenshot(),
        contentType: "image/png",
      });
      if (existsSync(auditPath)) {
        await testInfo.attach("native-request-audit", {
          path: auditPath,
          contentType: "application/x-ndjson",
        });
      }
    } finally {
      try {
        await client.close();
      } finally {
        await workspace.cleanup();
      }
    }
  }
});

test("refreshes reopened previews and observes a long unfinished turn read-only", async ({
  page,
}, testInfo) => {
  test.setTimeout(120_000);
  writeFileSync(statePath, "33");
  const workspace = await seedWorkspace({ repoPrefix: "ait-dsh-preview-opening-", git: false });
  const client = await connectDaemonClient<PreviewClient>({ clientIdPrefix: "dsh-opening" });
  try {
    await expect
      .poll(async () => {
        const snapshot = await client.getProvidersSnapshot({ cwd: workspace.repoPath });
        return snapshot.entries.find((entry) => entry.provider === "deepseek-harness")
          ?.supportsSessionPreview;
      })
      .toBe(true);
    const recent = await client.fetchRecentProviderSessions({
      cwd: workspace.repoPath,
      providers: ["deepseek-harness"],
    });
    const entry = recent.entries.find((item) => item.providerHandleId === "e2e-native-session");
    const running = recent.entries.find((item) => item.providerHandleId === "e2e-running-session");
    if (!entry || !running) throw new Error("Native preview fixtures were not listed");
    const before = await client.fetchProviderSessionPreview({ ...running, limit: 1 });
    expect(before.active).toBe(true);

    await gotoAppShell(page);
    await page.goto(buildHostWorkspaceRoute(getServerId(), workspace.workspaceId));
    await page.getByTestId("sidebar-import-session").click();
    const previewAction = page.getByTestId(
      `import-session-preview-deepseek-harness-${entry.providerHandleId}`,
    );
    await previewAction.click();
    const sheet = page.getByTestId("session-preview-sheet");
    const messages = sheet.getByText(/^(question|answer) \d+$/);
    await expect(page.getByText("answer 32", { exact: true })).toBeVisible();
    await expect(messages).toHaveCount(50);
    await page.keyboard.press("Escape");
    await expect(previewAction).toBeVisible();
    writeFileSync(statePath, "34");
    await previewAction.click();
    await expect(page.getByText("answer 33", { exact: true })).toBeVisible();
    await expect(messages).toHaveCount(50);
    await page.keyboard.press("Escape");
    await page
      .getByTestId(`import-session-preview-deepseek-harness-${running.providerHandleId}`)
      .click();
    await expect(page.getByTestId("session-preview-active-notice")).toBeVisible();
    await expect(page.getByText("running progress 19", { exact: true })).toBeVisible();
    const after = await client.fetchProviderSessionPreview({ ...running, limit: 1 });
    expect(after).toEqual({ ...before, requestId: after.requestId });
    expectNoWriterRequests("reopen and long unfinished-turn preview");
  } finally {
    try {
      await testInfo.attach("opening-before-cleanup", {
        body: await page.screenshot(),
        contentType: "image/png",
      });
      if (existsSync(auditPath)) {
        await testInfo.attach("native-request-audit", {
          path: auditPath,
          contentType: "application/x-ndjson",
        });
      }
    } finally {
      try {
        await client.close();
      } finally {
        await workspace.cleanup();
      }
    }
  }
});
