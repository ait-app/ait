import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import test from "node:test";
import yaml from "yaml";

const workflow = yaml.parse(
  await readFile(new URL("../.github/workflows/nightly.yml", import.meta.url), "utf8"),
);
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const context = { repo: { owner: "ait-app", repo: "ait" }, sha: "current", runId: 42 };

async function runScript(step, github, outputs = {}) {
  await new AsyncFunction("github", "context", "core", step.with.script)(github, context, {
    notice() {},
    setOutput(key, value) {
      outputs[key] = value;
    },
  });
  return outputs;
}

test("dev pushes replace builds per platform without cancelling publication", () => {
  assert.deepEqual(workflow.on.push.branches, ["dev"]);
  assert.equal(workflow.concurrency, undefined);
  assert.equal(workflow.jobs.prepare.if, "github.ref == 'refs/heads/dev'");
  assert.equal(workflow.jobs.prepare.outputs.current, "${{ steps.head.outputs.current }}");
  assert.equal(workflow.jobs.build.needs, "prepare");
  assert.equal(workflow.jobs.build.if, "needs.prepare.outputs.current == 'true'");
  assert.deepEqual(workflow.jobs.build.concurrency, {
    group: "nightly-dev-build-${{ matrix.platform }}",
    "cancel-in-progress": true,
  });
  const platforms = workflow.jobs.build.strategy.matrix.include.map((entry) => entry.platform);
  assert.equal(new Set(platforms).size, platforms.length);
  assert.equal(workflow.jobs.publish.needs, "build");
  assert.deepEqual(workflow.jobs.publish.concurrency, {
    group: "nightly-dev-publish",
    "cancel-in-progress": false,
  });
  assert.deepEqual(workflow.jobs.cleanup.needs, ["build", "publish"]);
  assert.match(workflow.jobs.cleanup.if, /always\(\)/);
  for (const job of Object.values(workflow.jobs)) {
    for (const step of job.steps) {
      if (!step.run) continue;
      const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
      assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
    }
  }
});

for (const sha of ["current", "newer"]) {
  test(`preparation admits only the dev head (${sha})`, async () => {
    const github = { rest: { repos: { getBranch: async () => ({ data: { commit: { sha } } }) } } };
    assert.deepEqual(await runScript(workflow.jobs.prepare.steps[0], github), {
      current: String(sha === context.sha),
    });
  });
}

const releaseStep = workflow.jobs.publish.steps.find((step) => step.id === "release");

test("a build superseded during compilation cannot touch the nightly release or tag", async () => {
  // No mutation APIs: any attempt to change the release fails this test.
  const github = {
    rest: { repos: { getBranch: async () => ({ data: { commit: { sha: "newer" } } }) } },
  };
  assert.deepEqual(await runScript(releaseStep, github), { publish: "false" });
  const publish = workflow.jobs.publish.steps.find(
    (step) => step.name === "Publish latest nightly",
  );
  assert.equal(publish.if, "steps.release.outputs.publish == 'true'");
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
    assert.deepEqual(await runScript(releaseStep, github), { publish: "true" });
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
  await runScript(workflow.jobs.cleanup.steps[0], github);
  assert.deepEqual(deleted, [
    { ...context.repo, artifact_id: 1 },
    { ...context.repo, artifact_id: 2 },
  ]);
});
