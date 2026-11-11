import { expect, test } from "../support/fixtures";
import { readFileSync, mkdtempSync } from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
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
const statePath = path.join(fixtureDirectory, "native-port");
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

interface PreviewClient {
  connect(): Promise<void>;
  close(): Promise<void>;
  getProvidersSnapshot(options?: { cwd?: string }): Promise<{
    entries: Array<{ provider: string; supportsSessionPreview?: boolean }>;
  }>;
  fetchRecentProviderSessions(options?: {
    cwd?: string;
    providers?: string[];
    limit?: number;
  }): Promise<{
    entries: Array<{
      providerId: string;
      providerHandleId: string;
      cwd: string;
      title: string | null;
    }>;
  }>;
  fetchProviderSessionPreview(options: {
    providerId: string;
    providerHandleId: string;
    cwd: string;
    cursor?: string;
    limit?: number;
  }): Promise<{
    entries: Array<{ key: string; item: { type: string; text?: string } }>;
    olderCursor: string | null;
    hasOlder: boolean;
    active: boolean;
  }>;
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
  "session/selectModel",
  "commands/execute",
  "session/new",
  "session/load",
];

function expectNoWriterRequests(scope: string) {
  const offenders = readAudit().filter((entry) => WRITER_METHODS.includes(entry.method));
  expect(offenders, `${scope}: native writer requests while browsing`).toEqual([]);
}

test("previews inactive DSH history read-only with fixed-cut pagination", async ({ page }) => {
  test.setTimeout(180_000);
  const workspace = await seedWorkspace({ repoPrefix: "ait-dsh-preview-", git: false });
  // The fixture Host lists its session under the cwd the daemon launches it with.
  const client = await connectDaemonClient<PreviewClient>({ clientIdPrefix: "dsh-preview" });
  try {
    // Provider discovery advertises the read-only preview capability for DSH.
    const snapshot = await client.getProvidersSnapshot({ cwd: workspace.repoPath });
    const dsh = snapshot.entries.find((entry) => entry.provider === "deepseek-harness");
    expect(dsh?.supportsSessionPreview).toBe(true);

    const recent = await client.fetchRecentProviderSessions({
      cwd: workspace.repoPath,
      providers: ["deepseek-harness"],
      limit: 20,
    });
    const entry = recent.entries.find((item) => item.providerId === "deepseek-harness");
    expect(entry).toBeTruthy();
    if (!entry) throw new Error("DSH session was not listed");

    // Typed SDK reads: newest page, then the older page on the same fixed cut.
    const first = await client.fetchProviderSessionPreview({
      providerId: entry.providerId,
      providerHandleId: entry.providerHandleId,
      cwd: entry.cwd,
      limit: 4,
    });
    expect(first.entries.map((item) => item.item.text)).toEqual([
      "answer 5",
      "question 5",
      "answer 4",
      "question 4",
    ]);
    expect(first.hasOlder).toBe(true);
    expect(first.olderCursor).toBeTruthy();
    expect(first.active).toBe(false);
    const second = await client.fetchProviderSessionPreview({
      providerId: entry.providerId,
      providerHandleId: entry.providerHandleId,
      cwd: entry.cwd,
      cursor: first.olderCursor ?? undefined,
      limit: 100,
    });
    expect(second.entries).toHaveLength(8);
    expect(second.hasOlder).toBe(false);
    expectNoWriterRequests("typed SDK preview reads");
    const pages = readAudit().filter((item) => item.method === "session/page");
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
    await expect(page.getByText("answer 5", { exact: true })).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId("session-preview-active-notice")).toHaveCount(0);
    await page.getByTestId("session-preview-load-older").click();
    await expect(page.getByText("question 0", { exact: true })).toBeVisible({ timeout: 30_000 });
    expectNoWriterRequests("browser preview browsing");

    // Closing the preview returns to the import sheet without importing anything.
    await page.keyboard.press("Escape");
    await expect(row).toBeVisible({ timeout: 30_000 });
    expectNoWriterRequests("after closing preview");
  } finally {
    await client.close().catch(() => undefined);
    await workspace.cleanup();
    await rm(fixtureDirectory, { recursive: true, force: true }).catch(() => undefined);
  }
});
