import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import test from "node:test";
import yaml from "yaml";
import control from "./nightly-control.cjs";
import { recordPrBuild } from "./pr-build-info.mjs";

const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8"),
);
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const context = { repo: { owner: "ait-app", repo: "ait" }, sha: "current", runId: 42 };

async function runScript(step, github, outputs = {}, controller = control) {
  await new AsyncFunction("github", "context", "core", "require", step.with.script)(
    github,
    context,
    {
      notice() {},
      setOutput(key, value) {
        outputs[key] = value;
      },
    },
    (name) => {
      assert.equal(name, "./scripts/nightly-control.cjs");
      return controller;
    },
  );
  return outputs;
}

test("main builds run independently while admission and publication share a queued lock", () => {
  assert.deepEqual(workflow.on.push.branches, ["main"]);
  assert.equal(workflow.concurrency, undefined);
  assert.equal(
    workflow.jobs["nightly-prepare"].if,
    "github.event_name != 'pull_request' && github.ref == 'refs/heads/main'",
  );
  assert.equal(
    workflow.jobs["nightly-prepare"].outputs.admitted,
    "${{ steps.window.outputs.admitted }}",
  );
  assert.equal(workflow.jobs.desktop.needs, "nightly-prepare");
  assert.equal(
    workflow.jobs.desktop.if,
    "${{ !cancelled() && (github.event_name == 'pull_request' || needs.nightly-prepare.outputs.admitted == 'true') }}",
  );
  assert.equal(workflow.jobs.desktop.concurrency, undefined);
  assert.deepEqual(
    workflow.jobs["nightly-prepare"].concurrency,
    workflow.jobs["nightly-publish"].concurrency,
  );
  assert.equal(workflow.jobs["nightly-prepare"].permissions.actions, "write");
  assert.equal(workflow.jobs["nightly-publish"].permissions.actions, "write");
  const platforms = workflow.jobs.desktop.strategy.matrix.include.map((entry) => entry.platform);
  assert.equal(new Set(platforms).size, platforms.length);
  assert.deepEqual(workflow.jobs["nightly-publish"].needs, ["desktop", "rust", "ui", "docs"]);
  assert.deepEqual(workflow.jobs["nightly-publish"].concurrency, {
    group: "nightly-main-control",
    queue: "max",
    "cancel-in-progress": false,
  });
  assert.deepEqual(workflow.jobs["nightly-cleanup"].needs, ["desktop", "nightly-publish"]);
  assert.match(workflow.jobs["nightly-cleanup"].if, /always\(\)/);
  for (const job of Object.values(workflow.jobs)) {
    for (const step of job.steps) {
      if (!step.run) continue;
      const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
      assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
    }
  }
});

function condition(expression, github, needs = {}, cancelled = false) {
  const code = expression
    .replace(/^\s*\$\{\{|\}\}\s*$/g, "")
    .replaceAll("needs.nightly-prepare", 'needs["nightly-prepare"]');
  return new Function(
    "github",
    "needs",
    "cancelled",
    "always",
    "contains",
    "fromJSON",
    `return (${code});`,
  )(
    github,
    needs,
    () => cancelled,
    () => true,
    (list, value) => list.includes(value),
    JSON.parse,
  );
}

test("PR merge builds are read-only and never schedule, publish, or clean up nightly", () => {
  assert.ok(Object.hasOwn(workflow.on, "pull_request"));
  assert.equal(workflow.on.pull_request_target, undefined);
  assert.deepEqual(workflow.permissions, { contents: "read" });
  assert.deepEqual(workflow.jobs.desktop.permissions, { contents: "read" });
  const github = { event_name: "pull_request", ref: "refs/pull/123/merge" };
  assert.equal(condition(workflow.jobs.desktop.if, github), true);
  for (const job of ["nightly-prepare", "nightly-publish", "nightly-cleanup"])
    assert.equal(condition(workflow.jobs[job].if, github), false);
  assert.equal(condition(workflow.jobs.desktop.if, github, {}, true), false);
  const upload = workflow.jobs.desktop.steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.with.overwrite, true);
  assert.match(upload.with.name, /pull_request.number/);
  assert.match(upload.with.name, /pull_request.head.sha/);
  assert.equal(
    upload.with["retention-days"],
    "${{ github.event_name == 'pull_request' && 14 || 1 }}",
  );
  const checkout = workflow.jobs.desktop.steps.find((step) =>
    step.uses?.startsWith("actions/checkout@"),
  );
  assert.deepEqual(checkout.with, { "persist-credentials": false }); // Default checkout tests the merge commit.
  assert.doesNotMatch(
    JSON.stringify(workflow.jobs.desktop),
    /secrets\.|GH_TOKEN|EXPO_TOKEN|eas build/,
  );
});

test("main publishing requires desktop and applicable CI checks to pass", () => {
  const github = { event_name: "push", ref: "refs/heads/main" };
  const needs = {
    desktop: { result: "success" },
    docs: { result: "success" },
    rust: { result: "skipped" },
    ui: { result: "success" },
  };
  assert.equal(condition(workflow.jobs["nightly-publish"].if, github, needs), true);
  for (const job of ["desktop", "docs", "rust", "ui"]) {
    assert.equal(
      condition(workflow.jobs["nightly-publish"].if, github, {
        ...needs,
        [job]: { result: "failure" },
      }),
      false,
    );
  }
  assert.equal(
    condition(workflow.jobs["nightly-publish"].if, { ...github, ref: "refs/heads/dev" }, needs),
    false,
  );
});

test("mobile publishing remains manual only", async () => {
  for (const name of ["release-android", "release-android-play", "release-ios-testflight"]) {
    const mobile = yaml.parse(
      await readFile(new URL(`../.github/workflows/${name}.yml`, import.meta.url), "utf8"),
    );
    assert.deepEqual(Object.keys(mobile.on), ["workflow_dispatch"]);
  }
});

test("PR package records both source and merge commits and checksums all downloaded files", async (t) => {
  const directory = await mkdtemp(path.join(tmpdir(), "ait-pr-assets-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await writeFile(path.join(directory, "Ait.dmg"), "test package");
  const env = {
    PR_NUMBER: "123",
    PR_HEAD_SHA: "a".repeat(40),
    PR_BASE_SHA: "b".repeat(40),
    GITHUB_SHA: "c".repeat(40),
    PLATFORM: "mac",
    GITHUB_SERVER_URL: "https://github.com",
    GITHUB_REPOSITORY: "ait-app/ait",
    GITHUB_RUN_ID: "456",
  };
  const info = await recordPrBuild(directory, env);
  assert.equal(info.pullRequest, 123);
  assert.equal(info.headCommit, env.PR_HEAD_SHA);
  assert.equal(info.sourceCommit, env.GITHUB_SHA);
  assert.equal(info.baseCommit, env.PR_BASE_SHA);
  assert.equal(info.channel, "pull-request");
  const sums = await readFile(path.join(directory, "SHA256SUMS"), "utf8");
  for (const file of ["Ait.dmg", "BUILD-INFO.json"]) {
    const hash = createHash("sha256")
      .update(await readFile(path.join(directory, file)))
      .digest("hex");
    assert.ok(sums.includes(`${hash}  ${file}\n`));
  }
  await assert.rejects(
    recordPrBuild(directory, { ...env, GITHUB_SHA: "unknown" }),
    /Invalid GITHUB_SHA/,
  );
});

const releaseStep = workflow.jobs["nightly-publish"].steps.find((step) => step.id === "release");

test("a rejected publication cannot touch the release or tag", async () => {
  assert.deepEqual(await runScript(releaseStep, {}, {}, { canPublish: async () => false }), {
    publish: "false",
  });
  const publish = workflow.jobs["nightly-publish"].steps.find(
    (step) => step.name === "Publish latest nightly",
  );
  assert.equal(publish.if, "steps.release.outputs.publish == 'true'");
  assert.match(publish.run, /<!-- ait-nightly-run: \$GITHUB_RUN_NUMBER -->/);
  const cancelIndex = workflow.jobs["nightly-publish"].steps.findIndex(
    (step) => step.name === "Cancel older builds after successful publication",
  );
  assert.equal(workflow.jobs["nightly-publish"].steps[cancelIndex - 1], publish);
  assert.equal(workflow.jobs["nightly-publish"].steps[cancelIndex].if, publish.if);
});

for (const existing of [true, false]) {
  test(`current build publishes with an ${existing ? "existing" : "absent"} nightly tag`, async () => {
    const calls = [];
    const lookup = async (data) => {
      if (!existing) throw Object.assign(new Error("Not found"), { status: 404 });
      return { data };
    };
    const github = {
      rest: {
        repos: {
          getBranch: async () => ({ data: { commit: { sha: context.sha } } }),
          getReleaseByTag: () => lookup({ id: 9 }),
          deleteRelease: async (args) => calls.push(["delete", args]),
        },
        git: {
          getRef: () => lookup({}),
          updateRef: async (args) => calls.push(["update", args]),
          createRef: async (args) => calls.push(["create", args]),
        },
      },
    };
    assert.deepEqual(await runScript(releaseStep, github, {}, { canPublish: async () => true }), {
      publish: "true",
    });
    assert.deepEqual(
      calls,
      existing
        ? [
            ["delete", { ...context.repo, release_id: 9 }],
            ["update", { ...context.repo, ref: "tags/nightly", sha: context.sha, force: true }],
          ]
        : [["create", { ...context.repo, ref: "refs/tags/nightly", sha: context.sha }]],
    );
  });
}

test("cleanup queries only its own run and preserves unrelated artifacts", async () => {
  const deleted = [];
  const listWorkflowRunArtifacts = Symbol("run artifacts endpoint");
  const github = {
    rest: {
      actions: {
        listWorkflowRunArtifacts,
        deleteArtifact: async (args) => deleted.push(args),
      },
    },
    paginate: async (endpoint, args) => {
      assert.equal(endpoint, listWorkflowRunArtifacts);
      assert.deepEqual(args, { ...context.repo, run_id: context.runId, per_page: 100 });
      return [
        { id: 1, name: "nightly-linux" },
        { id: 2, name: "nightly-mac" },
        { id: 3, name: "diagnostics" },
      ];
    },
  };
  await runScript(workflow.jobs["nightly-cleanup"].steps[0], github);
  assert.deepEqual(deleted, [
    { ...context.repo, artifact_id: 1 },
    { ...context.repo, artifact_id: 2 },
  ]);
});

function simulation() {
  const state = { runs: [], published: 0, cancelled: [], cancelError: null, releaseError: null };
  const endpoints = { listWorkflowRuns: Symbol("list workflow runs") };
  const github = {
    rest: {
      actions: {
        ...endpoints,
        cancelWorkflowRun: async ({ run_id }) => {
          if (state.cancelError) throw state.cancelError;
          const run = state.runs.find((entry) => entry.id === run_id);
          assert.ok(run);
          state.cancelled.push(run.run_number);
          run.status = "completed";
          run.conclusion = "cancelled";
        },
      },
      repos: {
        getReleaseByTag: async () => {
          if (state.releaseError) throw state.releaseError;
          if (!state.published) throw Object.assign(new Error("Not found"), { status: 404 });
          return {
            data: {
              body: `<!-- ait-nightly-run: ${state.published} --> <!-- ait-nightly-workflow: ci.yml -->`,
            },
          };
        },
      },
    },
    paginate: async (endpoint, args) => {
      assert.equal(endpoint, endpoints.listWorkflowRuns);
      assert.deepEqual(args, {
        ...context.repo,
        workflow_id: "ci.yml",
        branch: "main",
        per_page: 100,
      });
      // Deliberately return runs in arrival order, not sorted newest first.
      return state.runs.map((run) => ({ ...run }));
    },
  };
  function api(number) {
    return {
      github,
      context: { ...context, runId: number * 10, runNumber: number },
      core: { notice() {} },
    };
  }
  function add(number, overrides = {}) {
    state.runs.push({
      id: number * 10,
      run_number: number,
      head_branch: "main",
      head_sha: String(number).padStart(40, "0"),
      event: "push",
      status: "in_progress",
      ...overrides,
    });
  }
  async function publish(number) {
    assert.equal(await control.canPublish(api(number)), true);
    state.published = number;
    state.runs.find((run) => run.run_number === number).status = "completed";
    await control.cancelOlderBuilds(api(number));
  }
  return { state, api, add, publish };
}

test("1 and 2 coexist; 1 finishing first publishes, then 2 replaces it", async () => {
  const { state, api, add, publish } = simulation();
  add(1);
  assert.equal(await control.admitBuild(api(1)), true);
  add(2);
  assert.equal(await control.admitBuild(api(2)), true);
  assert.deepEqual(state.cancelled, []);
  await publish(1);
  assert.equal(state.published, 1);
  assert.deepEqual(state.cancelled, []);
  await publish(2);
  assert.equal(state.published, 2);
});

test("2 finishing first publishes and cancels 1; a late 1 cannot roll back nightly", async () => {
  const { state, api, add, publish } = simulation();
  add(1);
  add(2);
  assert.equal(await control.admitBuild(api(2)), true);
  await publish(2);
  assert.deepEqual(state.cancelled, [1]);
  assert.equal(await control.canPublish(api(1)), false);
  assert.equal(await control.admitBuild(api(1)), false);
  assert.equal(state.published, 2);
});

for (const first of [2, 3]) {
  test(`3 arriving cancels only 1; retained pair supports ${first} finishing first`, async () => {
    const { state, api, add, publish } = simulation();
    add(1);
    add(2);
    await control.admitBuild(api(2));
    add(3);
    assert.equal(await control.admitBuild(api(3)), true);
    assert.deepEqual(state.cancelled, [1]);
    assert.equal(await control.canPublish(api(1)), false);
    await publish(first);
    if (first === 2) {
      assert.deepEqual(state.cancelled, [1]);
      await publish(3);
    } else {
      assert.deepEqual(state.cancelled, [1, 2]);
      assert.equal(await control.canPublish(api(2)), false);
    }
    assert.equal(state.published, 3);
  });
}

test("burst arrivals and out-of-order preparation retain only the newest two", async () => {
  const { state, api, add } = simulation();
  for (let number = 1; number <= 5; number++) add(number);
  add(50, { head_branch: "dev" });
  add(51, { event: "pull_request" });
  assert.equal(await control.admitBuild(api(4)), true);
  assert.deepEqual(state.cancelled.sort(), [1, 2, 3]);
  assert.equal(await control.admitBuild(api(1)), false);
  assert.equal(await control.admitBuild(api(5)), true);
  assert.equal(await control.canPublish(api(4)), true);
});

test("a newer build failing leaves the older retained build publishable", async () => {
  const { state, api, add, publish } = simulation();
  add(1);
  add(2, { status: "completed", conclusion: "failure" });
  assert.equal(await control.admitBuild(api(1)), true);
  await publish(1);
  assert.equal(state.published, 1);
  assert.deepEqual(state.cancelled, []);
});

test("manual reruns keep their original order and never cancel a newer pair", async () => {
  const { state, api, add } = simulation();
  add(1, { run_attempt: 2 });
  add(2);
  add(3, { event: "workflow_dispatch" });
  assert.equal(await control.admitBuild(api(1)), false);
  assert.deepEqual(state.cancelled, []); // Current run exits through the build condition.
  assert.equal(await control.canPublish(api(1)), false);
});

test("legacy dev nightly migrates without mixing workflow run numbers", async () => {
  const { api, add } = simulation();
  add(1);
  add(2);
  const legacyApi = api(1);
  legacyApi.github.rest.repos.getReleaseByTag = async () => ({
    data: {
      body: `Development build from ${String(2).padStart(40, "0")}. Replaced on the next build.`,
    },
  });
  assert.equal(await control.canPublish(legacyApi), true);
  assert.equal(await control.canPublish(api(2)), true);
});

test("API errors fail closed; an already completed cancellation is harmless", async () => {
  const { state, api, add } = simulation();
  add(1);
  add(2);
  add(3);
  state.cancelError = Object.assign(new Error("Already finished"), { status: 409 });
  assert.equal(await control.admitBuild(api(3)), true);
  state.cancelError = Object.assign(new Error("Forbidden"), { status: 403 });
  await assert.rejects(control.admitBuild(api(3)), /Forbidden/);
  state.releaseError = Object.assign(new Error("Unavailable"), { status: 503 });
  await assert.rejects(control.canPublish(api(3)), /Unavailable/);
  assert.deepEqual(state.cancelled, []);
});
