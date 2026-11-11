import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { Page } from "@playwright/test";
import type { DaemonClient } from "@ait/client/internal/daemon-client";
import { expect, test } from "../support/fixtures";
import { repoRoot } from "../support/helpers/ait-daemon";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { fillQuestionAnswer, waitForQuestionPrompt } from "../support/helpers/questions";
import { seedWorkspace, type SeededWorkspace } from "../support/helpers/seed-client";

// Offline OMP RPC peer copied from the provider tests. The daemon resolves it through
// AIT_SERVER_OMP_BIN, and PI_CODING_AGENT_DIR keeps every native write in this temp root.
// `startup-dialog` makes the peer publish a dialog during initialization, before the first
// input is admitted — the startup-approval surface under test.
const agentDir = mkdtempSync(path.join(tmpdir(), "ait-e2e-omp-"));
const peerBinary = path.join(agentDir, "omp-peer");
const peerLog = path.join(agentDir, "calls.jsonl");
// Extend the existing offline peer with two independent dialogs. Keep the native
// turn open until both responses arrive, so answering one cannot settle the other.
const peerSource = readFileSync(
  path.join(repoRoot, "crates/provider/src/local/omp/tests/fixtures/omp.py"),
  "utf8",
);
const askBranch = "        if text == 'ask':";
const answerBranch = "    elif kind == 'extension_ui_response':";
if (!peerSource.includes(askBranch) || !peerSource.includes(answerBranch)) {
  throw new Error("OMP fixture interaction branches changed");
}
writeFileSync(
  peerBinary,
  peerSource
    .replace(
      askBranch,
      `        if text == 'two-dialogs':
            remaining_dialogs = {'first-question', 'second-question'}
            emit({'type':'extension_ui_request','id':'first-question','method':'input','title':'First question'})
            emit({'type':'extension_ui_request','id':'second-question','method':'input','title':'Second question','secret':True})
            continue
${askBranch}`,
    )
    .replace(
      answerBranch,
      `${answerBranch}
        if command.get('id') in ('first-question', 'second-question'):
            remaining_dialogs.discard(command['id'])
            if not remaining_dialogs:
                finish()
            continue`,
    ),
);
writeFileSync(path.join(agentDir, "startup-dialog"), "true");
chmodSync(peerBinary, 0o755);
mkdirSync(path.join(agentDir, "sessions", "workspace"), { recursive: true });
test.afterAll(async () => {
  await rm(agentDir, { recursive: true, force: true });
});

test.use({
  e2eDaemonEnvironment: {
    AIT_SERVER_OMP_BIN: peerBinary,
    PI_CODING_AGENT_DIR: agentDir,
  },
});

const STARTUP_QUESTION = "Startup input";

/** Native log size now; tests read only the entries their own session appends. */
function markPeerLog(): number {
  return existsSync(peerLog) ? statSync(peerLog).size : 0;
}

/** Commands the peer recorded after `fromByte`, in arrival order. */
function peerCommands(fromByte: number): Array<Record<string, unknown>> {
  if (!existsSync(peerLog)) return [];
  return readFileSync(peerLog, "utf8")
    .slice(fromByte)
    .split("\n")
    .filter((line) => line.trim().length > 0)
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

function peerAnswers(commands: Array<Record<string, unknown>>, id: string) {
  return commands.filter((entry) => entry.type === "extension_ui_response" && entry.id === id);
}

async function seedOmpAgent(input: {
  repoPrefix: string;
  title: string;
  model?: string;
  initialPrompt?: string;
}): Promise<SeededWorkspace & { agentId: string; logOffset: number }> {
  const workspace = await seedWorkspace({ repoPrefix: input.repoPrefix });
  try {
    const logOffset = markPeerLog();
    const agent = await workspace.client.createAgent({
      provider: "omp",
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: input.title,
      model: input.model,
      initialPrompt: input.initialPrompt,
    });
    return { ...workspace, agentId: agent.id, logOffset };
  } catch (error) {
    await workspace.cleanup();
    throw error;
  }
}

/** Answer the pre-initialization dialog the peer raises for every OMP session. */
async function answerStartupDialog(page: Page): Promise<void> {
  await waitForQuestionPrompt(page, 120_000);
  const card = page.getByTestId("question-form-card").first();
  await expect(card.getByTestId("question-form-current-question")).toHaveText(STARTUP_QUESTION);
  await fillQuestionAnswer(page, { question: STARTUP_QUESTION, answer: "approved" });
  await card.getByTestId("question-form-primary-action").click();
  await expect(page.getByTestId("question-form-card")).toHaveCount(0, { timeout: 30_000 });
}

test.describe("OMP structured interactions", () => {
  test("publishes the startup approval before admission and releases the queued input once", async ({
    page,
  }) => {
    test.setTimeout(180_000);

    const workspace = await seedOmpAgent({
      repoPrefix: "omp-startup-approval-",
      title: "OMP startup approval",
      initialPrompt: "startup release",
    });

    try {
      await openAgentRoute(page, {
        workspaceId: workspace.workspaceId,
        agentId: workspace.agentId,
      });

      await waitForQuestionPrompt(page, 120_000);
      await expect(
        page
          .getByTestId("question-form-card")
          .first()
          .getByTestId("question-form-current-question"),
      ).toHaveText(STARTUP_QUESTION);
      // Published before native initialization finishes: the queued first input has not
      // reached the provider and the dialog has no answer yet.
      expect(
        peerCommands(workspace.logOffset).filter((entry) => entry.type === "prompt"),
      ).toHaveLength(0);
      expect(peerAnswers(peerCommands(workspace.logOffset), "startup-choice")).toHaveLength(0);

      await answerStartupDialog(page);
      const answered = peerCommands(workspace.logOffset);
      expect(peerAnswers(answered, "startup-choice")).toEqual([
        expect.objectContaining({ value: "approved" }),
      ]);

      // Adoption releases the queued input exactly once through the same native session.
      await expect(page.getByText("Hello world", { exact: true })).toBeVisible({
        timeout: 60_000,
      });
      await expect
        .poll(
          () =>
            peerCommands(workspace.logOffset).filter(
              (entry) => entry.type === "prompt" && entry.message === "startup release",
            ).length,
        )
        .toBe(1);
      await expect(page.getByTestId("question-form-card")).toHaveCount(0);
    } finally {
      await workspace.cleanup();
    }
  });

  test("surfaces one terminal failure when queued startup input is rejected", async ({ page }) => {
    test.setTimeout(180_000);

    // The native peer rejects this prompt only after the startup dialog is answered.
    // This exercises normal failure settlement after provisional admission.
    const workspace = await seedOmpAgent({
      repoPrefix: "omp-startup-failure-",
      title: "OMP startup failure",
      initialPrompt: "reject",
    });

    try {
      await openAgentRoute(page, {
        workspaceId: workspace.workspaceId,
        agentId: workspace.agentId,
      });

      await answerStartupDialog(page);
      const settled = await workspace.client.waitForFinish(workspace.agentId, 60_000);
      expect(settled.status).toBe("error");
      const failure = settled.final?.lastError;
      if (!failure) throw new Error("Failed startup did not retain its error");
      await expect(page.getByText(failure, { exact: true })).toBeVisible({ timeout: 60_000 });
      await expect(page.getByText(failure, { exact: true })).toHaveCount(1);
      await expect(page.getByTestId("question-form-card")).toHaveCount(0);
      expect(
        peerCommands(workspace.logOffset).filter(
          (entry) => entry.type === "prompt" && entry.message === "reject",
        ),
      ).toHaveLength(1);
    } finally {
      await workspace.cleanup();
    }
  });

  test("answers one of two pending interactions and cancels the other", async ({ page }) => {
    test.setTimeout(180_000);

    const workspace = await seedOmpAgent({
      repoPrefix: "omp-ask-cancel-",
      title: "OMP concurrent questions",
    });

    const legacy = await connectDaemonClient<DaemonClient>({ clientIdPrefix: "legacy-question" });
    try {
      await openAgentRoute(page, {
        workspaceId: workspace.workspaceId,
        agentId: workspace.agentId,
      });
      await answerStartupDialog(page);

      await workspace.client.sendAgentMessage(workspace.agentId, "two-dialogs");
      await waitForQuestionPrompt(page, 120_000);
      const cards = page.getByTestId("question-form-card");
      await expect(cards).toHaveCount(2);
      const first = cards.filter({ hasText: "First question" });
      const second = cards.filter({ hasText: "Second question" });
      await expect(second.locator("input")).toHaveAttribute("type", "password");
      const get = await legacy.fetchAgent({ agentId: workspace.agentId });
      const list = await legacy.fetchAgents({ scope: "active" });
      const subscribed = await legacy.fetchAgents({ scope: "active", subscribe: {} });
      const waiting = await legacy.waitForFinish(workspace.agentId, 30_000);
      for (const [source, agent] of Object.entries({
        get: get?.agent,
        list: list.entries.find((entry) => entry.agent.id === workspace.agentId)?.agent,
        subscription: subscribed.entries.find((entry) => entry.agent.id === workspace.agentId)
          ?.agent,
        wait: waiting.final,
      })) {
        const pending = agent?.pendingPermissions ?? [];
        expect(pending.find((request) => request.id === "first-question")?.kind).toBe("question");
        const secret = pending.find((request) => request.id === "second-question");
        expect(secret?.kind, source).toBe("tool");
        expect(secret?.description).toContain("Upgrade");
        expect(secret?.input).toBeUndefined();
        expect(secret?.actions).toEqual([{ id: "deny", label: "Cancel", behavior: "deny" }]);
      }
      await first.locator("input").fill("approved");
      await first.getByTestId("question-form-primary-action").click();
      await expect(first).toHaveCount(0);
      await expect(second).toBeVisible();
      await expect
        .poll(() => peerAnswers(peerCommands(workspace.logOffset), "first-question"))
        .toEqual([expect.objectContaining({ value: "approved" })]);
      expect(peerAnswers(peerCommands(workspace.logOffset), "second-question")).toEqual([]);
      await second.locator("input").fill("must not be submitted");
      await second.getByTestId("question-form-dismiss").click();
      await expect(cards).toHaveCount(0);
      await expect
        .poll(() => peerAnswers(peerCommands(workspace.logOffset), "second-question"))
        .toEqual([expect.objectContaining({ cancelled: true })]);
      const cancelled = peerAnswers(peerCommands(workspace.logOffset), "second-question")[0];
      expect(cancelled.value).toBeUndefined();
      expect(cancelled.answers).toBeUndefined();
      expect((await workspace.client.waitForFinish(workspace.agentId, 30_000)).status).toBe("idle");
    } finally {
      await legacy.close();
      await workspace.cleanup();
    }
  });
});
