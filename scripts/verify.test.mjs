import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import yaml from "yaml";
import { planVerification } from "./verify.mjs";

const crate = (name, dependencies = [], hasLib = true, dir = `crates/${name}`) => ({
  name,
  dir,
  hasLib,
  dependencies,
});
const graph = [
  crate("domain"),
  crate("persistence", ["domain"]),
  crate("provider", ["domain"]),
  // A dev-only edge still means this crate's tests compile against persistence.
  crate("bot", ["provider", "persistence"]),
  crate("voice"),
  crate("daemon", ["bot", "provider", "voice"], false, "bins/daemon"),
];
const plan = (changed, options = {}) =>
  planVerification({ changed, graph, oxfmtVersion: "1.2.3", ...options });
const commands = (result, group) =>
  result.steps.filter((step) => step.group === group).map((step) => step.command.join(" "));
const find = (result, text) =>
  result.steps.map((step) => step.command.join(" ")).find((command) => command.includes(text));

test("docs and formatting run for every change, because a rename can break any link", () => {
  const result = plan(["docs/README.md"]);
  assert.deepEqual(commands(result, "docs"), ["npm run check:docs"]);
  assert.deepEqual(commands(result, "format"), ["npx --yes oxfmt@1.2.3 --check ."]);
  assert.deepEqual(result.outputs, { rust: false, ui: false, web_client: false });
});

test("owners scope tests only the owning crate but lints its dependents", () => {
  const result = plan(["crates/voice/src/lib.rs"]);
  assert.match(find(result, "nextest"), / -p voice --no-tests=pass$/);
  assert.match(find(result, "clippy"), / -p daemon -p voice --all-targets/);
  assert.match(find(result, "--doc"), / -p voice$/);
});

test("closure scope follows dev-dependency edges to every dependent", () => {
  const result = plan(["crates/persistence/src/lib.rs"], { scope: "closure" });
  assert.match(find(result, "nextest"), / -p bot -p daemon -p persistence --no-tests=pass$/);
});

test("doctests are limited to crates with a library target", () => {
  const result = plan(["bins/daemon/src/main.rs"]);
  assert.match(find(result, "nextest"), / -p daemon /);
  assert.equal(find(result, "--doc"), undefined);
});

test("Markdown inside a crate needs no Rust checks", () => {
  assert.equal(plan(["crates/voice/README.md"]).outputs.rust, false);
});

test("inputs compiled from outside a crate select the crates that read them", () => {
  const result = plan(["skills/ait-orchestration/SKILL.md"]);
  assert.match(find(result, "nextest"), / -p daemon -p provider /);
});

test("workspace-wide inputs lint everything locally and leave full tests to an explicit request", () => {
  const result = plan(["Cargo.lock"]);
  assert.match(find(result, "clippy"), /--workspace/);
  assert.equal(find(result, "nextest"), undefined);
  assert.match(result.notes[0], /--scope all/);
});

test("workspace-wide inputs run the whole suite in pull requests", () => {
  const result = plan(["rust-toolchain.toml"], { scope: "closure" });
  assert.match(find(result, "nextest"), /--workspace/);
  assert.match(find(result, "--doc"), /--workspace$/);
});

test("the pipeline's own files exercise Rust in CI but not locally", () => {
  for (const file of [".github/workflows/ci.yml", "scripts/verify.mjs"]) {
    assert.equal(plan([file]).outputs.rust, false, file);
    assert.equal(plan([file], { scope: "closure" }).outputs.rust, true, file);
  }
});

test("a crate path with no known owner fails open to the whole workspace", () => {
  const result = plan(["crates/brand-new/src/lib.rs"], { scope: "closure" });
  assert.match(find(result, "nextest"), /--workspace/);
});

test("CI selects the ci nextest profile", () => {
  const result = plan(["crates/voice/src/lib.rs"], { ci: true });
  assert.match(find(result, "nextest"), /--profile ci$/);
});

test("an unknown diff runs every group in full", () => {
  const result = plan(null, { scope: "closure" });
  assert.deepEqual(result.outputs, { rust: true, ui: true, web_client: true });
  assert.match(find(result, "nextest"), /--workspace/);
  assert.ok(find(result, "build:desktop-main"));
});

test("static Rust checks run before the long compile", () => {
  const steps = commands(plan(["crates/voice/src/lib.rs"]), "rust");
  assert.ok(steps[0].startsWith("cargo fmt"));
  assert.ok(
    steps.findIndex((step) => step.includes("python3")) <
      steps.findIndex((step) => step.includes("clippy")),
  );
  assert.ok(
    steps.findIndex((step) => step.includes("clippy")) <
      steps.findIndex((step) => step.includes("nextest")),
  );
});

test("pull requests keep the whole UI suite, with builds before the suites that import them", () => {
  const result = plan(["apps/desktop/src/main.ts"], { scope: "closure", ci: true, base: "abc" });
  const steps = commands(result, "ui");
  assert.ok(steps.includes("npm run check:ui -- abc"));
  assert.ok(!steps.includes("npm run check:ui"));
  assert.ok(steps.includes("npm run test:release"));
  assert.ok(steps.some((step) => /--workspace=@ait\/mobile -- .*src\/timeline/.test(step)));
  assert.ok(steps.includes("node --test scripts/verify.test.mjs"));
  assert.ok(
    steps.indexOf("npm run build:desktop-main") <
      steps.findIndex((step) => step.includes("--workspace=@ait/client")),
  );
});

test("local UI checks stay within the changed client", () => {
  const steps = commands(plan(["apps/desktop/src/main.ts"], { base: "abc" }), "ui");
  assert.ok(steps.includes("npm test --workspace=@ait/desktop"));
  assert.ok(steps.includes("npm run typecheck --workspace=@ait/desktop"));
  assert.ok(!steps.some((step) => step.includes("@ait/mobile")));
  assert.ok(!steps.includes("npm run test:release"));
  // Both commits and uncommitted edits need linting.
  assert.ok(steps.includes("npm run check:ui -- abc"));
  assert.ok(steps.includes("npm run check:ui"));
});

test("local mobile changes run CI's held-green suites plus the tests that changed", () => {
  const changedTest = "apps/mobile/src/runtime/host.test.ts";
  const source = "apps/mobile/src/runtime/host.ts";
  const result = plan([changedTest, source], { exists: () => true });
  const command = find(result, "--workspace=@ait/mobile --");
  assert.match(command, /src\/timeline/);
  assert.match(command, / src\/runtime\/host\.test\.ts$/);
  assert.doesNotMatch(command, /host\.ts/);
});

test("an unmapped client fails open to the whole UI suite", () => {
  const result = plan(["apps/site/index.ts"]);
  assert.equal(result.outputs.ui, true);
  assert.ok(find(result, "npm run test:release"));
});

test("package changes typecheck both clients and test the package", () => {
  const steps = commands(plan(["packages/client/src/index.ts"]), "ui");
  assert.ok(steps.includes("npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile"));
  assert.ok(steps.includes("npm run test --workspace=@ait/client"));
});

test("a script change runs its sibling test", () => {
  const exists = (file) => file === "scripts/check-docs.test.mjs";
  const result = plan(["scripts/check-docs.mjs"], { exists });
  assert.ok(find(result, "node --test scripts/check-docs.test.mjs"));
});

test("provider fixtures shared with the TypeScript clients select both sides", () => {
  const result = plan(["crates/provider-omp/tests/fixtures/omp_question.json"]);
  assert.equal(result.outputs.rust, true);
  assert.ok(find(result, "--workspace=@ait/client"));
});

test("a worktree without dependencies installs them before running UI checks", () => {
  const steps = commands(plan(["apps/desktop/src/main.ts"], { hasNodeModules: false }), "ui");
  assert.equal(steps[0], "npm ci");
});

test("--only limits execution without changing what the CI gate reports", () => {
  const result = plan(["crates/voice/src/lib.rs"], { only: ["docs"] });
  assert.deepEqual(
    result.steps.map((step) => step.group),
    ["docs"],
  );
  assert.equal(result.outputs.rust, true);
});

test("the web client gate covers its sources and skips documentation", () => {
  assert.equal(plan(["apps/mobile/src/app.ts"]).outputs.web_client, true);
  assert.equal(plan([".github/workflows/release.yml"]).outputs.web_client, true);
  assert.equal(plan(["apps/mobile/README.md"]).outputs.web_client, false);
  assert.equal(plan(["apps/desktop/src/main.ts"]).outputs.web_client, false);
});

test("Ait workspaces and CI run this same script", () => {
  const config = JSON.parse(readFileSync(new URL("../ait.json", import.meta.url), "utf8"));
  assert.match(config.scripts.verify.command, /node scripts\/verify\.mjs/);
  assert.match(config.worktree.setup, /scripts\/seed-worktree-target\.mjs/);
  // ait.json is copied onto branches that may predate the setup script.
  assert.match(config.worktree.setup, /^test ! -f scripts\/seed-worktree-target\.mjs \|\| /);

  const ci = yaml.parse(
    readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8"),
  );
  for (const job of ["changes", "lint", "rust", "ui"]) {
    const runs = ci.jobs[job].steps.map((step) => step.run ?? "");
    assert.ok(
      runs.some((run) => run.includes("node scripts/verify.mjs")),
      job,
    );
  }
});
