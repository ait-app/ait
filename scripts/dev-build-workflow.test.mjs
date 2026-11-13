import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import yaml from "yaml";
import control from "./dev-build-control.cjs";
import { devBuilderArgs } from "./dev-build.mjs";

const readWorkflow = async (name) =>
  yaml.parse(await readFile(new URL(`../.github/workflows/${name}`, import.meta.url), "utf8"));
const ci = await readWorkflow("ci.yml");
const devBuild = await readWorkflow("dev-build.yml");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const context = { repo: { owner: "ait-app", repo: "ait" }, sha: "a".repeat(40), runId: 42 };

test("CI delegates its checks to the shared verify script with a full-history diff base", () => {
  const verify = (job) =>
    ci.jobs[job].steps.find((step) => step.run?.includes("node scripts/verify.mjs"));
  assert.match(verify("lint").run, /--only docs,format --scope all$/);
  assert.match(verify("changes").run, /--scope closure --github-output$/);
  for (const [job, group] of [
    ["rust", "rust"],
    ["ui", "ui"],
  ]) {
    assert.match(verify(job).run, new RegExp(`--only ${group} --base "\\$VERIFY_BASE"`));
    assert.match(verify(job).env.VERIFY_BASE, /pull_request.base.sha.*github.event.before/);
    assert.match(verify(job).env.VERIFY_SCOPE, /'push' && 'all' \|\| 'closure'/);
  }
  for (const job of ["changes", "rust", "ui"]) {
    const checkout = ci.jobs[job].steps.find((step) => step.uses === "actions/checkout@v5");
    assert.equal(checkout.with["fetch-depth"], 0, job);
  }
  assert.ok(ci.jobs.ui.steps.some((step) => step.run === "npm ci"));
});

test("dev Linux desktop builds its daemon on the release glibc baseline", () => {
  const steps = devBuild.jobs.desktop.steps;
  const mac = steps.find((step) => step.name === "Build daemon");
  assert.equal(mac.if, "matrix.platform == 'mac'");
  const linux = steps.find((step) => step.name === "Build daemon (glibc 2.28 baseline)");
  assert.equal(linux.if, "matrix.platform == 'linux'");
  assert.match(linux.run, /^bash scripts\/build-linux-daemon\.sh daemon daemon /);
  // The container must use the same pinned toolchain as the macOS runner.
  const toolchain = steps.find((step) => step.uses?.startsWith("dtolnay/rust-toolchain@"));
  assert(linux.run.endsWith(` ${toolchain.uses.split("@")[1]}`));
  const build = steps.find((step) => step.name === "Build desktop");
  assert.match(build.env.AITD_BIN, /target\/\$\{\{ matrix.rust_target \}\}\/release\/daemon$/);
});

test("dev builds package both standalone daemons and explain the macOS quarantine", () => {
  const steps = devBuild.jobs.desktop.steps;
  const names = steps.map((step) => step.name);
  const pack = steps.find((step) => step.name === "Package standalone daemon");
  assert.match(pack.env.DAEMON_BIN, /target\/\$\{\{ matrix.rust_target \}\}\/release\/daemon$/);
  assert.match(pack.run, /release-assets\.mjs daemon .* release-assets --dev "\$DEV_BUILD_LABEL"/);
  assert.match(pack.run, /codesign --verify/);
  // The archive must be in release-assets before the artifact upload picks it up.
  const upload = steps.findIndex((step) => step.uses?.startsWith("actions/upload-artifact@"));
  assert(names.indexOf("Package standalone daemon") < upload);
  const publish = devBuild.jobs.publish.steps.find(
    (step) => step.name === "Publish latest dev build",
  );
  assert.match(publish.run, /xattr -d com\.apple\.quarantine aitd/);
});

// Evaluates a job condition like GitHub: without a status function, success() is implied.
function condition(expression, github, needs = {}, cancelled = false) {
  let code = expression
    .replace(/^\s*\$\{\{|\}\}\s*$/g, "")
    .replaceAll("needs.*.result", "Object.values(needs).map((need) => need.result)")
    .replace(/needs\.([\w-]+)/g, (_, job) => `needs[${JSON.stringify(job)}]`);
  if (!/\b(?:always|cancelled|success|failure)\(/.test(code)) {
    const succeeded = !cancelled && Object.values(needs).every((need) => need.result === "success");
    code = `${succeeded} && (${code})`;
  }
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

async function runScript(step, github, warnings = []) {
  await new AsyncFunction("github", "context", "core", "require", step.with.script)(
    github,
    context,
    {
      notice() {},
      warning(message) {
        warnings.push(message);
      },
    },
    (name) => {
      assert.equal(name, "./scripts/dev-build-control.cjs");
      return control;
    },
  );
  return warnings;
}

test("CI only checks code: it never packages, publishes or writes to the repository", () => {
  assert.deepEqual(Object.keys(ci.on).sort(), ["pull_request", "push"]);
  assert.deepEqual(ci.permissions, { contents: "read" });
  for (const [name, job] of Object.entries(ci.jobs)) {
    assert.doesNotMatch(name, /nightly|dev|desktop|publish/);
    assert.notEqual(job.permissions?.contents, "write", name);
    assert.equal(job.permissions?.actions, undefined, name);
  }
  assert.doesNotMatch(JSON.stringify(ci), /dev-build-control|gh release|upload-artifact/);
});

test("dev builds run only when triggered manually, one at a time, only from main", () => {
  assert.deepEqual(Object.keys(devBuild.on), ["workflow_dispatch"]);
  assert.equal(devBuild.on.workflow_dispatch, null);
  assert.deepEqual(devBuild.permissions, { contents: "read" });
  assert.deepEqual(devBuild.concurrency, { group: "dev-build", "cancel-in-progress": false });
  assert.equal(devBuild.jobs.prepare.if, "github.ref == 'refs/heads/main'");
  assert.deepEqual(devBuild.jobs.prepare.permissions, { contents: "read", actions: "read" });
  assert.deepEqual(devBuild.jobs.desktop.permissions, { contents: "read" });
  assert.deepEqual(devBuild.jobs.publish.permissions, { contents: "write", actions: "write" });
  const platforms = devBuild.jobs.desktop.strategy.matrix.include.map((entry) => entry.platform);
  assert.deepEqual(platforms, ["linux", "mac"]);
  assert.doesNotMatch(
    JSON.stringify(devBuild.jobs.desktop),
    /secrets\.|GH_TOKEN|EXPO_TOKEN|eas build/,
  );
  for (const job of Object.values(devBuild.jobs)) {
    for (const step of job.steps) {
      if (!step.run) continue;
      const result = spawnSync("bash", ["-n", "-c", step.run], { encoding: "utf8" });
      assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
    }
  }
});

test("packaging and publication follow an admitted prepare job only", () => {
  for (const [publish, admitted] of [
    ["true", true],
    ["false", false],
    [undefined, false],
  ]) {
    const prepare = { prepare: { result: "success", outputs: { publish } } };
    assert.equal(condition(devBuild.jobs.desktop.if, {}, prepare), admitted);
    const needs = { ...prepare, desktop: { result: "success" } };
    assert.equal(condition(devBuild.jobs.publish.if, {}, needs), admitted);
  }
  const admitted = { prepare: { result: "success", outputs: { publish: "true" } } };
  for (const result of ["failure", "cancelled", "skipped"]) {
    const needs = { ...admitted, desktop: { result } };
    assert.equal(condition(devBuild.jobs.publish.if, {}, needs), false, result);
  }
  assert.equal(condition(devBuild.jobs.desktop.if, {}, admitted, true), false);
  assert.deepEqual(devBuild.jobs.publish.needs, ["prepare", "desktop"]);
  const upload = devBuild.jobs.desktop.steps.find((step) =>
    step.uses?.startsWith("actions/upload-artifact@"),
  );
  assert.equal(upload.with.name, "dev-${{ needs.prepare.outputs.label }}-${{ matrix.platform }}");
  assert.equal(upload.with["retention-days"], 1);
  // Artifact cleanup is the last publication step, so a failed publish keeps both platforms.
  const steps = devBuild.jobs.publish.steps;
  assert.equal(steps.at(-1).name, "Remove temporary dev build artifacts");
  assert.equal(steps.at(-1).if, undefined);
});

// A fake GitHub API whose CI listing returns `runs` in turn, or fails with `listError`.
function admission({ runs = [[]], listError = null } = {}) {
  const listed = [];
  let call = 0;
  const github = {
    rest: {
      actions: {
        listWorkflowRuns: async (args) => {
          listed.push(args);
          if (listError) throw listError;
          return { data: { workflow_runs: runs[Math.min(call++, runs.length - 1)] } };
        },
      },
    },
  };
  return { github, listed };
}

const run = (overrides) => ({
  run_number: 1,
  head_branch: "main",
  status: "completed",
  conclusion: "success",
  ...overrides,
});

async function decide(state, options = {}) {
  const warnings = [];
  const notices = [];
  const waits = [];
  const publish = await control.admit({
    github: state.github,
    context,
    core: { notice: (message) => notices.push(message), warning: (m) => warnings.push(m) },
    wait: async (milliseconds) => waits.push(milliseconds),
    attempts: 3,
    ...options,
  });
  return { publish, warnings, notices, waits };
}

test("passing CI on main's commit admits the build", async () => {
  const state = admission({ runs: [[run()]] });

  const result = await decide(state);

  assert.deepEqual(result, { publish: true, warnings: [], notices: [], waits: [] });
  assert.deepEqual(state.listed, [
    {
      ...context.repo,
      workflow_id: "ci.yml",
      head_sha: context.sha,
      event: "push",
      per_page: 20,
    },
  ]);
});

test("a running or not yet listed CI run is awaited before admission", async () => {
  const state = admission({
    runs: [[], [run({ status: "in_progress", conclusion: null })], [run()]],
  });

  const result = await decide(state);

  assert.equal(result.publish, true);
  assert.deepEqual(result.waits, [30_000, 30_000]);
});

test("failed, cancelled or never finishing CI keeps the previous dev build", async () => {
  for (const runs of [
    [[run({ conclusion: "failure" })]],
    [[run({ conclusion: "cancelled" })]],
    [[run({ status: "queued", conclusion: null })]],
  ]) {
    const result = await decide(admission({ runs }));
    assert.equal(result.publish, false);
    assert.equal(result.warnings.length, 1);
    assert.match(result.warnings[0], /previous dev build stays published/);
  }
});

test("the newest main push decides; reruns and other branches do not mask it", async () => {
  const runs = [
    [
      run({ run_number: 7, conclusion: "failure" }),
      run({ run_number: 9, head_branch: "feature" }),
      run({ run_number: 8 }),
    ],
  ];
  assert.equal((await decide(admission({ runs }))).publish, true);
  const failing = [[run({ run_number: 8, conclusion: "failure" }), run({ run_number: 7 })]];
  assert.equal((await decide(admission({ runs: failing }))).publish, false);
});

test("CI lookup errors fail the run instead of guessing", async () => {
  const error = Object.assign(new Error("Server error"), { status: 500 });
  await assert.rejects(decide(admission({ listError: error })), error);
});

const notFound = () => Object.assign(new Error("Not found"), { status: 404 });
const ruleViolation = () =>
  Object.assign(
    new Error("Repository rule violations found\n\nCannot update this protected ref.\n\n"),
    { status: 422 },
  );
const releaseStep = devBuild.jobs.publish.steps.find(
  (step) => step.name === "Move tag and remove previous dev release",
);

// Record ref and release mutations in order; `tag` is the current tag commit or null.
function publication({ tag, release = true, updateError = null }) {
  const calls = [];
  const github = {
    rest: {
      repos: {
        getReleaseByTag: async () => {
          if (!release) throw notFound();
          return { data: { id: 9 } };
        },
        deleteRelease: async (args) => calls.push(["delete", args]),
      },
      git: {
        getRef: async () => {
          if (tag === null) throw notFound();
          return { data: { object: { sha: tag } } };
        },
        updateRef: async (args) => {
          if (updateError) throw updateError;
          calls.push(["update", args]);
        },
        createRef: async (args) => calls.push(["create", args]),
      },
    },
  };
  return { calls, github };
}

const update = ["update", { ...context.repo, ref: "tags/dev", sha: context.sha, force: true }];
const create = ["create", { ...context.repo, ref: "refs/tags/dev", sha: context.sha }];
const removeRelease = ["delete", { ...context.repo, release_id: 9 }];

for (const [name, state, expected] of [
  ["an older dev tag is moved before", { tag: "previous" }, [update, removeRelease]],
  ["an absent dev tag is created before", { tag: null }, [create, removeRelease]],
  ["a tag already on this build is kept before", { tag: context.sha }, [removeRelease]],
  ["a first publication creates the tag without", { tag: null, release: false }, [create]],
]) {
  test(`${name} replacing the release`, async () => {
    const { calls, github } = publication(state);

    const warnings = await runScript(releaseStep, github);

    assert.deepEqual(calls, expected);
    assert.deepEqual(warnings, []);
  });
}

test("a ruleset-protected tag stays put while the release still publishes this build", async () => {
  const { calls, github } = publication({ tag: "previous", updateError: ruleViolation() });

  const warnings = await runScript(releaseStep, github);

  assert.deepEqual(calls, [removeRelease]);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /stays at previous/);
  assert.match(warnings[0], new RegExp(`publishes ${context.sha}`));
  const publish = devBuild.jobs.publish.steps.find(
    (step) => step.name === "Publish latest dev build",
  );
  assert.match(publish.run, /--verify-tag/);
  assert.match(publish.run, /Development build from \$GITHUB_SHA/);
});

test("other tag failures stop before the published release is deleted", async () => {
  for (const updateError of [
    Object.assign(new Error("Server error"), { status: 500 }),
    Object.assign(new Error("Validation failed"), { status: 422 }),
  ]) {
    const { calls, github } = publication({ tag: "previous", updateError });

    await assert.rejects(runScript(releaseStep, github), updateError);
    assert.deepEqual(calls, []);
  }
});

test("published notes record the source commit", () => {
  const publish = devBuild.jobs.publish.steps.find(
    (step) => step.name === "Publish latest dev build",
  );
  const body = publish.run.match(/--notes "([^"]*)"/)[1].replaceAll("$GITHUB_SHA", context.sha);
  assert.match(body, /Development build from ([0-9a-f]{40})\b/);
});

test("Google Play publishing only runs from the release workflow", async () => {
  const directory = new URL("../.github/workflows/", import.meta.url);
  const publishers = [];
  for (const name of await readdir(directory)) {
    const text = await readFile(new URL(name, directory), "utf8");
    if (text.includes("upload-google-play")) publishers.push(name);
  }
  assert.deepEqual(publishers, ["release.yml"]);
});

test("packaging shell passes dev build names literally", async (t) => {
  const directory = await mkdtemp(path.join(tmpdir(), "ait-dev-args-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await writeFile(
    path.join(directory, "npm"),
    "#!/usr/bin/env node\nconsole.log(JSON.stringify(process.argv.slice(2)));\n",
    { mode: 0o755 },
  );
  const step = devBuild.jobs.desktop.steps.find((step) => step.name === "Package desktop preview");
  for (const platform of ["linux", "mac"]) {
    for (const label of ["abcdef01-2026-10-08", "01234567-2026-11-05"]) {
      const result = spawnSync("bash", ["-e", "-o", "pipefail", "-c", step.run], {
        cwd: new URL("../apps/desktop", import.meta.url),
        env: {
          ...process.env,
          PATH: `${directory}:${process.env.PATH}`,
          DEV_BUILD_LABEL: label,
          PLATFORM: platform,
          ARCH: platform === "mac" ? "arm64" : "x64",
          GITHUB_REPOSITORY: "ait-app/ait",
          GITHUB_REPOSITORY_OWNER: "ait-app",
        },
        encoding: "utf8",
      });
      assert.equal(result.status, 0, result.stderr);
      const args = JSON.parse(result.stdout);
      for (const arg of devBuilderArgs(label)) assert.ok(args.includes(arg), arg);
    }
  }
  assert.equal(devBuild.jobs.desktop.env.DEV_BUILD_LABEL, "${{ needs.prepare.outputs.label }}");
  assert.equal(
    devBuild.jobs.publish.env.DEV_BUILD_LABEL,
    devBuild.jobs.desktop.env.DEV_BUILD_LABEL,
  );
  const publish = devBuild.jobs.publish.steps.find(
    (step) => step.name === "Publish latest dev build",
  );
  assert.match(publish.run, /--title "Ait Dev \$DEV_BUILD_LABEL"/);
});

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
        { id: 1, name: "dev-01234567-2026-10-08-linux" },
        { id: 2, name: "dev-01234567-2026-10-08-mac" },
        { id: 3, name: "diagnostics" },
      ];
    },
  };
  const cleanup = devBuild.jobs.publish.steps.find(
    (step) => step.name === "Remove temporary dev build artifacts",
  );
  await runScript(cleanup, github);
  assert.deepEqual(deleted, [
    { ...context.repo, artifact_id: 1 },
    { ...context.repo, artifact_id: 2 },
  ]);
});
