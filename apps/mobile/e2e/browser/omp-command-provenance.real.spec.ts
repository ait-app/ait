import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, test } from "../support/fixtures";
import {
  expectComposerDraft,
  expectComposerVisible,
  fillComposerDraft,
  submitMessage,
} from "../support/helpers/composer";
import { openAgentRoute } from "../support/helpers/mock-agent";
import { seedWorkspace } from "../support/helpers/seed-client";
import { getServerId } from "../support/helpers/server-id";

// Real installed OMP resolved from PATH. No model inference: the exercised paths are native
// catalog discovery, same-name plugin dispatch, builtin refusal and metadata-only hints.
// Each plugin echoes the argument it received, so the timeline proves both which owner ran
// the command and that no hint text ever entered the submitted draft.
const agentDir = mkdtempSync(path.join(tmpdir(), "ait-omp-commands-agent-"));
mkdirSync(path.join(agentDir, "extensions"), { recursive: true });
writeFileSync(
  path.join(agentDir, "models.yml"),
  [
    "providers:",
    "  ait-local-fixture:",
    "    baseUrl: http://127.0.0.1:9/v1",
    "    api: openai-completions",
    "    auth: none",
    "    models:",
    "      - id: fixture",
    "        name: Local fixture",
    "        reasoning: false",
    "        input: [text]",
    "        contextWindow: 32768",
    "        maxTokens: 1024",
    "        cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0}",
    "",
  ].join("\n"),
);
writeFileSync(
  path.join(agentDir, "extensions", "goal-plugin.ts"),
  [
    'import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";',
    "export default function (pi: ExtensionAPI) {",
    '  pi.registerCommand("goal", {',
    '    description: "Project goal plugin that shares a host feature name",',
    '    input: { hint: "<objective>" },',
    "    handler: async (args) => {",
    "      await pi.sendMessage({",
    '        customType: "goal-plugin",',
    "        content: `GOAL-PLUGIN-NATIVE-OK:${args}`,",
    "        display: true,",
    "      });",
    "    },",
    "  });",
    '  pi.registerCommand("new", {',
    '    description: "Legal collision with the Ait reset shortcut",',
    "    handler: async (args) => {",
    "      await pi.sendMessage({",
    '        customType: "new-plugin",',
    "        content: `NEW-PLUGIN-NATIVE-OK:${args}`,",
    "        display: true,",
    "      });",
    "    },",
    "  });",
    "}",
    "",
  ].join("\n"),
);
writeFileSync(
  path.join(agentDir, "extensions", "record-plugin.ts"),
  [
    'import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";',
    "export default function (pi: ExtensionAPI) {",
    '  pi.registerCommand("record", {',
    '    description: "Project recorder that shares a terminal-only command name",',
    "    handler: async (args) => {",
    "      await pi.sendMessage({",
    '        customType: "record-plugin",',
    "        content: `RECORD-PLUGIN-NATIVE-OK:${args}`,",
    "        display: true,",
    "      });",
    "    },",
    "  });",
    "}",
    "",
  ].join("\n"),
);

test.use({
  e2eDaemonEnvironment: {
    PI_CODING_AGENT_DIR: agentDir,
    OMP_PROFILE: "",
    PI_PROFILE: "",
  },
});

test.afterAll(() => {
  rmSync(agentDir, { recursive: true, force: true });
});

test("OMP command provenance: refusal, same-name plugin dispatch, metadata-only hints", async ({
  page,
}) => {
  test.setTimeout(240_000);
  const workspace = await seedWorkspace({ repoPrefix: "ait-omp-commands-", git: false });
  try {
    const agent = await workspace.client.createAgent({
      provider: "omp",
      model: '["ait-local-fixture","fixture"]',
      cwd: workspace.repoPath,
      workspaceId: workspace.workspaceId,
      title: "OMP command provenance",
    });
    await page.goto("/");
    await expect(
      page.getByTestId(`sidebar-workspace-row-${getServerId()}:${workspace.workspaceId}`),
    ).toBeVisible({ timeout: 30_000 });
    await openAgentRoute(page, { workspaceId: workspace.workspaceId, agentId: agent.id });
    await expectComposerVisible(page, { timeout: 90_000 });

    const popover = page.getByTestId("composer-autocomplete-popover");
    await fillComposerDraft(page, "/");
    await expect(popover.getByText("/compact", { exact: true }).first()).toBeVisible({
      timeout: 30_000,
    });
    await expect(popover.getByText("/model", { exact: true })).toHaveCount(0);

    // /new is legal in OMP 18.17.0 and also names Ait's destructive /clear alias.
    // Native source must survive both menu selection and bare-command submission.
    const nativeRoute = page.url();
    await fillComposerDraft(page, "/new");
    const newCommand = popover.getByText("/new", { exact: true }).first();
    await expect(newCommand).toBeVisible({ timeout: 30_000 });
    await expect(popover.getByText(/extension/).first()).toBeVisible();
    // Accept through the keyboard path: the composer's pointer accept is inert on the web
    // surface at this base commit (verified identical at 22efa58f), so the spec drives the
    // supported interaction instead of asserting a pre-existing defect.
    await newCommand.focus();
    await page.keyboard.press("Tab");
    await expectComposerDraft(page, "/new ");
    await submitMessage(page, "/new");
    await expect(page.getByText("NEW-PLUGIN-NATIVE-OK:", { exact: true })).toBeVisible({
      timeout: 60_000,
    });
    await expect(page).toHaveURL(nativeRoute);

    // The same catalog policy applies once the native session is live.
    await fillComposerDraft(page, "/");
    await expect(popover.getByText("/compact", { exact: true }).first()).toBeVisible();
    await expect(popover.getByText("/model", { exact: true })).toHaveCount(0);

    // A plugin that shares a blocked host feature name keeps its real native source.
    await fillComposerDraft(page, "/goal");
    await expect(popover.getByText("/goal", { exact: true }).first()).toBeVisible({
      timeout: 30_000,
    });
    await expect(popover.getByText("/model", { exact: true })).toHaveCount(0);

    // A plugin that claims a terminal-only builtin name dispatches natively and receives
    // exactly the typed argument; no builtin refusal substitutes for its execution.
    await submitMessage(page, "/record recorded-here");
    await expect(
      page.getByText("RECORD-PLUGIN-NATIVE-OK:recorded-here", { exact: true }),
    ).toBeVisible({ timeout: 60_000 });
    await expect(
      page.getByText("/record is available only in the OMP terminal.", { exact: false }),
    ).toHaveCount(0);

    // The plugin that shares the undelivered /goal feature name does the same, and the
    // submitted draft carries only the typed text — no metadata is substituted.
    await submitMessage(page, "/goal reach-the-goal");
    await expect(
      page.getByText("GOAL-PLUGIN-NATIVE-OK:reach-the-goal", { exact: true }),
    ).toBeVisible({ timeout: 60_000 });
    await expect(
      page.getByText("The builtin /goal command is not available in Ait yet.", { exact: false }),
    ).toHaveCount(0);

    // A genuinely builtin-owned managed name is refused locally: readable text, no model
    // turn and no navigation, because the runtime reports /model from the builtin source.
    await submitMessage(page, "/model fixture");
    await expect(
      page.getByText("The builtin /model command is disabled in Ait.", { exact: false }),
    ).toBeVisible({ timeout: 30_000 });

    // Argument hints come from native metadata and stay decorative: the installed runtime
    // reports its own hint for /compact, and the draft keeps only the command and separator.
    await fillComposerDraft(page, "/compact ");
    await expect(page.getByTestId("composer-command-argument-hint")).toContainText("soft", {
      timeout: 15_000,
    });
    await expectComposerDraft(page, "/compact ");
  } finally {
    await workspace.cleanup();
  }
});
