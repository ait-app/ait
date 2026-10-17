import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import yaml from "yaml";

const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8"),
);
const prCleanup = yaml.parse(
  await readFile(new URL("../.github/workflows/cancel-merged-pr-ci.yml", import.meta.url), "utf8"),
);

test("main and PR CI run checks without packaging desktop installers or publishing nightlies", () => {
  assert.ok(Object.hasOwn(workflow.on, "pull_request"));
  assert.deepEqual(workflow.on.push.branches, ["main"]);
  assert.deepEqual(Object.keys(workflow.jobs).sort(), [
    "changes",
    "docs",
    "format",
    "headless-macos",
    "rust",
    "ui",
  ]);
  assert.doesNotMatch(JSON.stringify(workflow), /electron-builder|release-assets|nightly-/);
});

test("UI CI keeps changed lint, formatting, and the shared client suite", () => {
  const steps = workflow.jobs.ui.steps;
  assert.equal(steps.find((step) => step.uses === "actions/checkout@v5").with["fetch-depth"], 0);
  const check = steps.find((step) => step.name === "Check changed UI lint");
  assert.equal(check.run, 'npm run check:ui -- "$UI_DIFF_BASE"');
  assert.match(check.env.UI_DIFF_BASE, /pull_request.base.sha.*github.event.before/);
  assert.ok(steps.some((step) => step.run === "node --test scripts/check-ui.test.mjs"));
  assert.match(workflow.jobs.format.steps.at(-1).run, /oxfmt@.*--check \./);
  assert.ok(steps.some((step) => step.run === "npm run test --workspace=@ait/client"));
});

test("tagged desktop releases still build Linux and Apple Silicon", async () => {
  const release = yaml.parse(
    await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8"),
  );
  assert.deepEqual(release.on.push.tags, ["v*"]);
  assert.deepEqual(
    release.jobs.build.strategy.matrix.include.map((entry) => entry.platform),
    ["linux", "mac"],
  );
});

test("merging a PR cancels its unfinished CI revisions", async () => {
  assert.deepEqual(prCleanup.on, {
    pull_request_target: { branches: ["main"], types: ["closed"] },
  });
  assert.deepEqual(prCleanup.permissions, { actions: "write" });
  const step = prCleanup.jobs.cancel.steps[0];
  assert.equal(step.uses, "actions/github-script@v8");
  assert.equal(step.run, undefined);

  const pr = { number: 123, head: { ref: "feature", repo: { id: 7 } } };
  const endpoint = Symbol("list workflow runs");
  const cancelled = [];
  const runs = [
    {
      id: 1,
      event: "pull_request",
      status: "queued",
      head_branch: "feature",
      head_repository: { id: 7 },
      pull_requests: [{ number: 123 }],
    },
    {
      id: 2,
      event: "pull_request",
      status: "in_progress",
      head_branch: "feature",
      head_repository: { id: 7 },
      pull_requests: [{ number: 123 }],
    },
    {
      id: 3,
      event: "push",
      status: "in_progress",
      head_branch: "main",
      head_repository: { id: 7 },
      pull_requests: [],
    },
    {
      id: 4,
      event: "pull_request",
      status: "in_progress",
      head_branch: "other",
      head_repository: { id: 7 },
      pull_requests: [{ number: 456 }],
    },
  ];

  await new (Object.getPrototypeOf(async function () {}).constructor)(
    "github",
    "context",
    "core",
    step.with.script,
  )(
    {
      async paginate(method, params) {
        assert.equal(method, endpoint);
        assert.equal(params.workflow_id, "ci.yml");
        assert.equal(params.branch, "feature");
        return runs.filter((run) => run.status === params.status);
      },
      rest: {
        actions: {
          listWorkflowRuns: endpoint,
          async cancelWorkflowRun({ run_id: runId }) {
            cancelled.push(runId);
          },
        },
      },
    },
    { repo: { owner: "ait-app", repo: "ait" }, payload: { pull_request: pr } },
    { notice() {} },
  );

  assert.deepEqual(cancelled, [1, 2]);
});

test("mobile publishing remains manual only", async () => {
  for (const name of ["release-android", "release-android-play", "release-ios-testflight"]) {
    const mobile = yaml.parse(
      await readFile(new URL(`../.github/workflows/${name}.yml`, import.meta.url), "utf8"),
    );
    assert.deepEqual(Object.keys(mobile.on), ["workflow_dispatch"]);
  }
});
