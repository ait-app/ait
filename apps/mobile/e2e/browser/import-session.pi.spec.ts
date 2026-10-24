import type { DaemonClient } from "@ait/client/internal/daemon-client";
import type { Locator } from "@playwright/test";
import { randomUUID } from "node:crypto";
import { appendFile, chmod, copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { buildHostWorkspaceRoute } from "@/utils/host-routes";
import { expect, test } from "../support/fixtures";
import {
  attachImageFromMenu,
  composerLocator,
  expectAttachmentPill,
} from "../support/helpers/composer";
import { connectDaemonClient } from "../support/helpers/daemon-client-loader";
import { reloadPreservingHostRegistry } from "../support/helpers/hosts";
import { startIsolatedHostDaemon } from "../support/helpers/isolated-host-daemon";
import { seedSavedSettingsHosts } from "../support/helpers/settings";
import { createTempDirectory, createTempGitRepo } from "../support/helpers/workspace";

const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
const LIVE_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVR4nGNgYPj/H4QBDfsD/Yde1YcAAAAASUVORK5CYII=";
const TITLE = "Pi import acceptance";
const PROMPT = "Review the retained Pi screenshot";
const SUMMARY = "Pi compacted context summary";
const EDITED = "Pi answer after context edit";
const FOLLOW_UP = "Continue the imported conversation";
const EXTERNAL = "Pi history added outside Ait";

for (const viewport of [
  { name: "desktop", width: 1280, height: 900 },
  { name: "mobile", width: 390, height: 844 },
]) {
  test(`imports and continues Pi history on ${viewport.name}, preserving it across restart and refresh`, async ({
    page,
  }, testInfo) => {
    test.setTimeout(120_000);
    const root = await createTempDirectory("ait-pi-import-");
    const repo = await createTempGitRepo("ait-pi-project-");
    let server: Awaited<ReturnType<typeof startIsolatedHostDaemon>> | undefined;
    let client: DaemonClient | undefined;
    let projectId: string | undefined;
    try {
      const sessionId = randomUUID();
      const store = path.join(root.path, "sessions");
      const file = path.join(store, `2026-10-20_${sessionId}.jsonl`);
      const rpcLog = path.join(root.path, "rpc.jsonl");
      const executable = path.join(root.path, "pi-rpc.mjs");
      await mkdir(store);
      await copyFile(path.resolve(__dirname, "../fixtures/pi-import-rpc.mjs"), executable);
      await chmod(executable, 0o755);
      const original = nativeHistory(sessionId, repo.path);
      await writeFile(file, original);
      await writeFile(rpcLog, "");
      server = await startIsolatedHostDaemon("pi-import", {
        environment: {
          NODE_ENV: "development",
          HOME: root.path,
          PI_CODING_AGENT_DIR: path.join(root.path, "pi"),
          PI_CODING_AGENT_SESSION_DIR: store,
          AIT_SERVER_PI_BIN: executable,
          PI_IMPORT_RPC_LOG: rpcLog,
        },
      });
      client = await connectDaemonClient<DaemonClient>({
        port: server.port,
        clientIdPrefix: "pi-import",
      });
      const created = await client.createWorkspace({
        source: { kind: "directory", path: repo.path },
      });
      expect(created.error).toBeNull();
      const workspace = created.workspace!;
      projectId = workspace.projectId;

      await page.setViewportSize(viewport);
      await seedSavedSettingsHosts(page, [
        {
          serverId: server.serverId,
          endpoint: `127.0.0.1:${server.port}`,
          label: "Pi import host",
        },
      ]);
      await page.goto(buildHostWorkspaceRoute(server.serverId, workspace.id));
      await page.getByTestId("workspace-header-menu-trigger").click();
      await page.getByTestId("workspace-header-import-agent").click();
      await expect(page.getByTestId("import-session-scope")).toHaveText("This workspace");
      const row = page.getByTestId(`import-session-session-pi-${sessionId}`);
      await expect(row).toBeVisible({ timeout: 30_000 });
      await expect(row).toContainText(TITLE);
      await expect(row).toContainText("Confirm the Pi import");
      await row.click();
      await expect(page.getByTestId("import-session-sheet")).toHaveCount(0);

      const agents = (await client.fetchAgents()).entries.filter(
        (entry) => entry.agent.provider === "pi",
      );
      expect(agents).toHaveLength(1);
      const agentId = agents[0]!.agent.id;
      expect(agents[0]!.agent).toMatchObject({ cwd: repo.path, workspaceId: workspace.id });
      await expect(page.getByTestId("user-message").filter({ hasText: PROMPT })).toBeVisible();
      for (const text of [SUMMARY, EDITED, "Pi imported answer"]) {
        await expect(page.getByTestId("assistant-message").filter({ hasText: text })).toBeVisible();
      }
      for (const text of ["Pi discarded prompt", "Pi discarded answer", "Pi stale answer"]) {
        await expect(page.getByText(text, { exact: true })).toHaveCount(0);
      }
      const importedUser = page.getByTestId("user-message").filter({ hasText: PROMPT });
      const liveUser = page.getByTestId("user-message").filter({ hasText: FOLLOW_UP });
      await expectImageAttachment(importedUser);
      await expect
        .poll(() => readdir(path.join(server!.dataDir, "agents/provider-images")))
        .toHaveLength(1);
      expect(await readFile(file, "utf8")).toBe(original);
      // Catalog processes may open, but listing/import must never start a native writer.
      const opens = (await readFile(rpcLog, "utf8"))
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line));
      expect(opens.every((entry) => entry.type === "open" && entry.sessionFile === null)).toBe(
        true,
      );

      await attachImageFromMenu(page, {
        name: "pi-follow-up.png",
        mimeType: "image/png",
        buffer: Buffer.from(LIVE_PNG, "base64"),
      });
      await expectAttachmentPill(page, "composer-image-attachment-pill");
      await composerLocator(page).fill(FOLLOW_UP);
      await page.getByRole("button", { name: "Send message", exact: true }).click();
      await expect(
        page.getByTestId("assistant-message").filter({ hasText: `Pi continued: ${FOLLOW_UP}` }),
      ).toBeVisible();
      expect((await client.waitForFinish(agentId, 15_000)).status).toBe("idle");
      await expectImageAttachment(liveUser, 2);
      const log = (await readFile(rpcLog, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      expect(log.filter((entry) => entry.type === "prompt")).toEqual([
        {
          type: "prompt",
          sessionId,
          sessionFile: file,
          text: FOLLOW_UP,
          images: [{ type: "image", mimeType: "image/png", data: LIVE_PNG }],
        },
      ]);
      const submittedRecords = (await readFile(file, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      expect(submittedRecords.at(-2).message).toEqual({
        role: "user",
        content: [
          { type: "text", text: FOLLOW_UP },
          { type: "image", mimeType: "image/png", data: LIVE_PNG },
        ],
      });
      expect(await readdir(store)).toEqual([path.basename(file)]);
      const beforeRestart = await client.fetchAgentTimeline(agentId);
      const serialized = JSON.stringify(beforeRestart.entries);
      expect(serialized).not.toContain(PNG);
      expect(serialized).not.toContain(LIVE_PNG);
      for (const text of ["Pi discarded prompt", "Pi discarded answer", "Pi stale answer"]) {
        expect(serialized).not.toContain(text);
      }

      await client.close();
      client = undefined;
      // Stop the owned writer before simulating an external Pi history update.
      const previousPid = server.getPid();
      await server.restart();
      expect(server.getPid()).not.toBe(previousPid);
      const records = (await readFile(file, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      const externalUser = randomUUID();
      await appendFile(
        file,
        jsonl([
          message(externalUser, records.at(-1)!.id, {
            role: "user",
            content: "Read external history",
          }),
          message(randomUUID(), externalUser, assistant(EXTERNAL)),
        ]),
      );
      client = await connectDaemonClient<DaemonClient>({
        port: server.port,
        clientIdPrefix: "pi-import-restarted",
      });
      expect((await client.fetchAgentTimeline(agentId)).entries).toEqual(beforeRestart.entries);
      await reloadPreservingHostRegistry(page);
      await expect(
        page.getByTestId("assistant-message").filter({ hasText: `Pi continued: ${FOLLOW_UP}` }),
      ).toBeVisible();
      await expect(page.getByText(EXTERNAL, { exact: true })).toHaveCount(0);
      await expectImageAttachment(liveUser, 2);

      await client.refreshAgent(agentId);
      await expect(
        page.getByTestId("assistant-message").filter({ hasText: EXTERNAL }),
      ).toBeVisible();
      await expect(
        page.getByTestId("assistant-message").filter({ hasText: `Pi continued: ${FOLLOW_UP}` }),
      ).toHaveCount(1);
      await expectImageAttachment(importedUser);
      await expectImageAttachment(liveUser, 2);
      await expect(page.getByRole("button", { name: "Open image attachment" })).toHaveCount(2);
      const refreshed = JSON.stringify((await client.fetchAgentTimeline(agentId)).entries);
      expect(refreshed).not.toContain(PNG);
      expect(refreshed).not.toContain(LIVE_PNG);
      expect(await readdir(store)).toEqual([path.basename(file)]);
    } catch (error) {
      await testInfo.attach("pi-import-before-cleanup", {
        body: await page.screenshot(),
        contentType: "image/png",
      });
      throw error;
    } finally {
      try {
        if (client && projectId) await client.removeProject(projectId);
      } finally {
        try {
          await client?.close();
        } finally {
          await server?.close();
          await Promise.all([repo.cleanup(), root.cleanup()]);
        }
      }
    }
  });
}

async function expectImageAttachment(userMessage: Locator, width = 1) {
  const image = userMessage.getByRole("button", { name: "Open image attachment" });
  await expect(image).toBeVisible();
  await expect(image).toHaveCount(1);
  await expect
    .poll(() =>
      image
        .locator("img")
        .evaluateAll(
          (images, expectedWidth) =>
            images.some(
              (image) =>
                image instanceof HTMLImageElement &&
                image.complete &&
                image.naturalWidth === expectedWidth,
            ),
          width,
        ),
    )
    .toBe(true);
}

function jsonl(records: unknown[]) {
  return records.map((record) => `${JSON.stringify(record)}\n`).join("");
}

function message(id: string, parentId: string | null, content: object) {
  return { type: "message", id, parentId, timestamp: "2026-10-20T00:00:00Z", message: content };
}

function assistant(text: string) {
  return {
    role: "assistant",
    provider: "e2e",
    model: "pi-import-model",
    stopReason: "stop",
    content: [{ type: "text", text }],
  };
}

function nativeHistory(sessionId: string, cwd: string) {
  return jsonl([
    { type: "session", version: 3, id: sessionId, cwd, timestamp: "2026-10-20T00:00:00Z" },
    message("old-user", null, { role: "user", content: "Pi discarded prompt" }),
    message("old-answer", "old-user", assistant("Pi discarded answer")),
    message("kept-user", "old-answer", {
      role: "user",
      content: [
        { type: "text", text: PROMPT },
        { type: "image", mimeType: "image/png", data: PNG },
      ],
    }),
    message("kept-answer", "kept-user", assistant("Pi stale answer")),
    {
      type: "compaction",
      id: "compact",
      parentId: "kept-answer",
      summary: SUMMARY,
      firstKeptEntryId: "kept-user",
      tokensBefore: 50_000,
      timestamp: "2026-10-20T00:00:00Z",
    },
    message("new-user", "compact", { role: "user", content: "Confirm the Pi import" }),
    message("new-answer", "new-user", assistant("Pi imported answer")),
    {
      type: "context_edit",
      id: "edit",
      parentId: "new-answer",
      targetId: "kept-answer",
      replacement: { content: EDITED },
      timestamp: "2026-10-20T00:00:00Z",
    },
    {
      type: "session_info",
      id: "info",
      parentId: "edit",
      name: TITLE,
      timestamp: "2026-10-20T00:00:00Z",
    },
  ]);
}
