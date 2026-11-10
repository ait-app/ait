// The one verification entry point: Ait workspaces, developers and CI all run this script.
//
// Scopes decide how far a change reaches:
//   owners  (default) tests only the crates that own a changed file, per AGENTS.md.
//   closure           also tests every crate that depends on them. Pull requests use this.
//   all               runs each selected group in full. Pushes to main use this.
import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const repository = fileURLToPath(new URL("../", import.meta.url));
const GROUPS = ["docs", "format", "rust", "ui"];
const SCOPES = ["owners", "closure", "all"];
const NULL_SHA = /^0+$/;

// These affect every crate's build or test settings.
const RUST_WIDE = [
  /^Cargo\.(?:toml|lock)$/,
  /^\.cargo\//,
  /^rust-toolchain/,
  /^\.?rustfmt\.toml$/,
  /^\.?clippy\.toml$/,
  /^\.config\/nextest\.toml$/,
];
// The pipeline itself: exercise it fully in CI, but editing it is no reason to compile locally.
const RUST_WIDE_IN_CI = [/^\.github\/workflows\/ci\.yml$/, /^scripts\/verify\.mjs$/];
// Files outside a crate that Rust compiles in or reads at test time.
const RUST_INPUTS = [
  [/^skills\/ait-orchestration\//, ["provider", "daemon"]],
  [/^scripts\/fixtures\//, ["api"]],
];

const UI_WIDE = [
  /^package(?:-lock)?\.json$/,
  /^\.npmrc$/,
  /^tsconfig\.base\.json$/,
  /^\.oxfmtrc\.json$/,
  /^patches\//,
  /^assets\//,
];
const UI_RELEASE =
  /^(?:\.github\/workflows\/|scripts\/(?:.*release|dev-build|source-version|verify-|prepare-desktop|edit-windows))/;
const UI_TESTED_PACKAGES = ["protocol", "client", "highlight"];
// Only these mobile suites are held green in CI; move additions here deliberately.
const MOBILE_SUITES = [
  "src/components/question-form-card-core.test.ts",
  "src/runtime/host-runtime.test.ts",
  "src/i18n/resources.test.ts",
  "src/runtime/daemon-transport",
  "maestro/support",
  "native-release-version.test.ts",
  "src/timeline",
  "src/types/stream.test.ts",
  "src/runtime/replica-cache",
  "src/runtime/directory-sync",
  "src/screens/workspace/workspace-route-state.test.ts",
  "src/runtime/online-service-host-sync.test.ts",
  "src/screens/settings/online-service-host-section.test.tsx",
  "src/utils/tool-call-parsers.test.ts",
  "src/utils/diff-highlight.test.ts",
  "src/voice/audio-engine.native.test.ts",
  "src/voice/audio-engine.web.test.ts",
  "src/voice/voice-runtime.test.ts",
];
const WEB_CLIENT =
  /^(?:apps\/mobile\/|packages\/|patches\/|assets\/|package(?:-lock)?\.json$|\.npmrc$|tsconfig\.base\.json$|\.github\/workflows\/(?:ci|release)\.yml$)/;

const isDoc = (file) => file.endsWith(".md");
const step = (group, ...command) => ({ group, command });
const matches = (patterns, file) => patterns.some((pattern) => pattern.test(file));

function ownerOf(file, graph) {
  return graph
    .filter((crate) => file.startsWith(`${crate.dir}/`))
    .toSorted((left, right) => right.dir.length - left.dir.length)[0]?.name;
}

/** Follows normal, build and dev dependency edges, since each can break a dependent's tests. */
function withDependents(names, graph) {
  const reached = new Set(names);
  for (let grew = true; grew; ) {
    grew = false;
    for (const crate of graph) {
      if (!reached.has(crate.name) && crate.dependencies.some((name) => reached.has(name))) {
        reached.add(crate.name);
        grew = true;
      }
    }
  }
  return reached;
}

function classifyRust(changed, graph, scope) {
  const owners = new Set();
  let wide = false;
  for (const file of changed) {
    if (matches(RUST_WIDE, file) || (scope !== "owners" && matches(RUST_WIDE_IN_CI, file))) {
      wide = true;
    }
    for (const [pattern, crates] of RUST_INPUTS) {
      if (pattern.test(file)) for (const crate of crates) owners.add(crate);
    }
    if (/^(?:bins|crates)\//.test(file) && !isDoc(file)) {
      const owner = ownerOf(file, graph);
      if (owner) owners.add(owner);
      else wide = true;
    }
  }
  return { owners, wide };
}

function rustSteps({ changed, graph, scope, ci }) {
  const { owners, wide } =
    scope === "all" ? { owners: new Set(), wide: true } : classifyRust(changed, graph, scope);
  if (!wide && owners.size === 0) return { steps: [], notes: [] };

  const everyCrate = graph.map((crate) => crate.name);
  const sorted = (names) => [...names].toSorted();
  const select = (names) =>
    names.length === graph.length ? ["--workspace"] : names.flatMap((name) => ["-p", name]);

  // Compiling dependents costs little and catches API breaks the owners' own tests cannot.
  const linted = wide ? everyCrate : sorted(withDependents(owners, graph));
  let tested;
  if (scope === "owners") tested = sorted(owners);
  else tested = wide ? everyCrate : sorted(withDependents(owners, graph));

  const steps = [
    step("rust", "cargo", "fmt", "--all", "--check"),
    // ADR-094: client method names must match the Rust component declarations.
    step("rust", "python3", "scripts/check-paseo-client-methods.py"),
    step("rust", "python3", "scripts/rust_method_specs_test.py"),
    step("rust", "python3", "scripts/check-crate-coverage.test.py"),
    step(
      "rust",
      "cargo",
      "clippy",
      "--locked",
      ...select(linted),
      "--all-targets",
      "--",
      "-D",
      "warnings",
    ),
  ];
  const notes = [];
  if (scope === "owners" && wide) {
    notes.push("Workspace-wide Rust inputs changed; run --scope all to test every crate.");
  }
  if (tested.length > 0) {
    steps.push(
      step(
        "rust",
        "cargo",
        "nextest",
        "run",
        "--locked",
        ...select(tested),
        "--no-tests=pass",
        ...(ci ? ["--profile", "ci"] : []),
      ),
    );
    const libraries = tested.filter((name) => graph.find((crate) => crate.name === name)?.hasLib);
    const selector =
      tested.length === graph.length ? ["--workspace"] : libraries.flatMap((name) => ["-p", name]);
    // nextest does not execute doctests, and cargo rejects --doc for a binary-only package.
    if (selector.length > 0)
      steps.push(step("rust", "cargo", "test", "--locked", "--doc", ...selector));
  }
  return { steps, notes };
}

function classifyUi(changed, exists) {
  const needs = {
    touched: false,
    wide: false,
    release: false,
    desktop: false,
    mobile: false,
    typecheck: new Set(),
    packages: new Set(),
    mobileTests: [],
    scriptTests: new Set(),
  };
  const typecheckClients = () => {
    needs.typecheck.add("desktop");
    needs.typecheck.add("mobile");
  };
  for (const file of changed) {
    if (matches(UI_WIDE, file)) {
      needs.touched = needs.wide = true;
      continue;
    }
    if (UI_RELEASE.test(file)) needs.touched = needs.release = true;
    if (file.startsWith("crates/provider/tests/fixtures/")) {
      needs.touched = true;
      for (const name of UI_TESTED_PACKAGES) needs.packages.add(name);
      typecheckClients();
    }
    if (file === "ait.json") {
      needs.touched = true;
      needs.scriptTests.add("scripts/verify.test.mjs");
    }
    if (isDoc(file)) continue;
    if (file.startsWith("apps/desktop/")) {
      needs.touched = needs.desktop = true;
      needs.typecheck.add("desktop");
    } else if (file.startsWith("apps/mobile/")) {
      needs.touched = needs.mobile = true;
      needs.typecheck.add("mobile");
      if (/\.test\.tsx?$/.test(file) && exists(file)) {
        needs.mobileTests.push(file.slice("apps/mobile/".length));
      }
    } else if (file.startsWith("apps/")) {
      // A client this script has no mapping for yet: check everything until it does.
      needs.touched = needs.wide = true;
    } else if (file.startsWith("packages/")) {
      needs.touched = true;
      typecheckClients();
      const name = file.split("/")[1];
      if (UI_TESTED_PACKAGES.includes(name)) needs.packages.add(name);
    } else if (file.startsWith("scripts/") || file.startsWith(".github/workflows/")) {
      needs.touched = true;
      for (const test of siblingTests(file, exists)) needs.scriptTests.add(test);
    }
  }
  return needs;
}

function siblingTests(file, exists) {
  if (!file.startsWith("scripts/") || file.startsWith("scripts/fixtures/")) return [];
  if (/\.test\.[cm]js$/.test(file)) return exists(file) ? [file] : [];
  const stem = file.replace(/\.[^./]+$/, "");
  return [`${stem}.test.mjs`, `${stem}.test.cjs`].filter(exists);
}

const CLIENT_WORKSPACES = { desktop: "@ait/desktop", mobile: "@ait/mobile" };

function uiSteps({ changed, scope, ci, base, exists, hasNodeModules }) {
  const needs = classifyUi(changed, exists);
  if (scope !== "all" && !needs.touched) return [];
  const full = scope !== "owners" || needs.wide;

  const steps = [];
  if (!hasNodeModules) steps.push(step("ui", "npm", "ci"));
  steps.push(
    step("ui", "npx", "oxlint"),
    step("ui", "node", "--test", "scripts/check-ui.test.mjs"),
  );
  if (base) steps.push(step("ui", "npm", "run", "check:ui", "--", base));
  // check:ui compares commits when given a base, so local edits need the working-tree pass too.
  if (!ci) steps.push(step("ui", "npm", "run", "check:ui"));

  if (full || needs.release) {
    for (const script of [
      "verify:local-packages",
      "verify:release",
      "test:release",
      "test:mobile-release",
    ]) {
      steps.push(step("ui", "npm", "run", script));
    }
  }
  const typecheck = full ? Object.keys(CLIENT_WORKSPACES) : [...needs.typecheck];
  const packages = full
    ? UI_TESTED_PACKAGES
    : UI_TESTED_PACKAGES.filter((name) => needs.packages.has(name));
  const desktop = full || needs.desktop;
  const mobile = full || needs.mobile;
  if (typecheck.length > 0 || packages.length > 0 || desktop || mobile) {
    // Also builds the SDK and highlight packages that the suites below import.
    steps.push(step("ui", "npm", "run", "build:desktop-main"));
  }
  if (typecheck.length > 0) {
    steps.push(
      step(
        "ui",
        "npm",
        "run",
        "typecheck",
        ...typecheck.map((name) => `--workspace=${CLIENT_WORKSPACES[name]}`),
      ),
    );
  }
  if (packages.length > 0) {
    steps.push(
      step("ui", "npm", "run", "test", ...packages.map((name) => `--workspace=@ait/${name}`)),
    );
  }
  if (desktop) steps.push(step("ui", "npm", "test", "--workspace=@ait/desktop"));
  if (mobile) {
    // `vitest related` would reach suites outside this list, some of which fail to load.
    const suites = full ? MOBILE_SUITES : [...MOBILE_SUITES, ...needs.mobileTests];
    steps.push(step("ui", "npm", "run", "test", "--workspace=@ait/mobile", "--", ...suites));
  }
  const scriptTests = new Set(needs.scriptTests);
  if (full) scriptTests.add("scripts/verify.test.mjs");
  if (scriptTests.size > 0)
    steps.push(step("ui", "node", "--test", ...[...scriptTests].toSorted()));
  return steps;
}

/**
 * Turns changed paths into ordered checks without touching the machine.
 * A null `changed` means the diff is unknown, which runs everything.
 */
export function planVerification({
  changed,
  graph,
  scope = "owners",
  only = GROUPS,
  ci = false,
  base = null,
  oxfmtVersion,
  exists = () => false,
  hasNodeModules = true,
}) {
  const effectiveScope = changed === null ? "all" : scope;
  const files = changed ?? [];
  const context = {
    changed: files,
    graph,
    scope: effectiveScope,
    ci,
    base,
    exists,
    hasNodeModules,
  };
  const rust = rustSteps(context);
  const ui = uiSteps(context);
  const planned = {
    docs: [step("docs", "npm", "run", "check:docs")],
    format: [step("format", "npx", "--yes", `oxfmt@${oxfmtVersion}`, "--check", ".")],
    rust: rust.steps,
    ui,
  };
  return {
    steps: GROUPS.filter((group) => only.includes(group)).flatMap((group) => planned[group]),
    notes: rust.notes,
    outputs: {
      rust: rust.steps.length > 0,
      ui: ui.length > 0,
      web_client: changed === null || files.some((file) => WEB_CLIENT.test(file) && !isDoc(file)),
    },
  };
}

function git(args, { allowFailure = false } = {}) {
  const result = spawnSync("git", args, { cwd: repository, encoding: "utf8" });
  if (result.status === 0) return result.stdout;
  if (allowFailure) return null;
  throw new Error(`git ${args.join(" ")} failed: ${result.stderr.trim()}`);
}

const splitPaths = (output) => output.split("\0").filter(Boolean);

function defaultBase() {
  for (const ref of ["origin/main", "main"]) {
    const base = git(["merge-base", "HEAD", ref], { allowFailure: true })?.trim();
    if (base) return base;
  }
  return null;
}

function changedFiles(base) {
  const files = [
    ...splitPaths(git(["diff", "--name-only", "--no-renames", "-z", "HEAD"])),
    ...splitPaths(git(["ls-files", "--others", "--exclude-standard", "-z"])),
  ];
  if (base)
    files.push(...splitPaths(git(["diff", "--name-only", "--no-renames", "-z", base, "HEAD"])));
  return [...new Set(files)];
}

function readCrateGraph() {
  const result = spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) throw new Error(`cargo metadata failed: ${result.stderr.trim()}`);
  const metadata = JSON.parse(result.stdout);
  const names = new Set(metadata.packages.map((crate) => crate.name));
  return metadata.packages.map((crate) => ({
    name: crate.name,
    dir: path.relative(metadata.workspace_root, path.dirname(crate.manifest_path)),
    hasLib: crate.targets.some((target) => target.kind.includes("lib")),
    dependencies: crate.dependencies
      .map((dependency) => dependency.name)
      .filter((name) => names.has(name)),
  }));
}

function parseOptions(argv) {
  const { values } = parseArgs({
    args: argv,
    options: {
      base: { type: "string" },
      scope: { type: "string", default: "owners" },
      only: { type: "string", default: GROUPS.join(",") },
      plan: { type: "boolean", default: false },
      "github-output": { type: "boolean", default: false },
    },
  });
  const only = values.only.split(",").filter(Boolean);
  const unknown = only.find((group) => !GROUPS.includes(group));
  if (unknown) throw new Error(`Unknown group "${unknown}"; expected ${GROUPS.join(", ")}.`);
  if (!SCOPES.includes(values.scope)) {
    throw new Error(`Unknown scope "${values.scope}"; expected ${SCOPES.join(", ")}.`);
  }
  return { ...values, only };
}

function resolveChanges(requestedBase, ci) {
  if (requestedBase === undefined) {
    const base = defaultBase();
    return { base, changed: changedFiles(base) };
  }
  const isNull = NULL_SHA.test(requestedBase);
  const known =
    requestedBase !== "" &&
    !isNull &&
    git(["rev-parse", "--verify", "--quiet", `${requestedBase}^{commit}`], {
      allowFailure: true,
    }) !== null;
  if (known) return { base: requestedBase, changed: changedFiles(requestedBase) };
  if (!ci && !isNull) throw new Error(`Unknown base revision "${requestedBase}".`);
  // A new branch, an overwritten push or a base-less event has no diff, so nothing may be skipped.
  return { base: null, changed: null };
}

function execute(steps) {
  for (const [index, { command }] of steps.entries()) {
    const label = command.join(" ");
    console.log(`\n==> [${index + 1}/${steps.length}] ${label}`);
    const started = Date.now();
    const result = spawnSync(command[0], command.slice(1), { cwd: repository, stdio: "inherit" });
    const seconds = ((Date.now() - started) / 1000).toFixed(1);
    if (result.error) throw result.error;
    if (result.status !== 0) {
      console.error(`\nverify failed after ${seconds}s: ${label}`);
      return result.status ?? 1;
    }
    console.log(`    passed in ${seconds}s`);
  }
  console.log(`\nverify passed: ${steps.length} checks.`);
  return 0;
}

function main(argv) {
  const options = parseOptions(argv);
  const ci = process.env.CI === "true";
  const { base, changed } = resolveChanges(options.base, ci);
  const manifest = JSON.parse(readFileSync(path.join(repository, "package.json"), "utf8"));
  // The CI gate only asks whether Rust files changed, so it needs no Rust toolchain.
  const needsGraph = !options["github-output"] && options.only.includes("rust");
  const plan = planVerification({
    changed,
    graph: needsGraph ? readCrateGraph() : [],
    scope: options.scope,
    only: options.only,
    ci,
    base,
    oxfmtVersion: manifest.devDependencies.oxfmt,
    exists: (file) => existsSync(path.join(repository, file)),
    hasNodeModules: existsSync(path.join(repository, "node_modules")),
  });

  if (options["github-output"]) {
    const lines = Object.entries(plan.outputs).map(([key, value]) => `${key}=${value}\n`);
    appendFileSync(process.env.GITHUB_OUTPUT, lines.join(""));
    process.stdout.write(lines.join(""));
    return 0;
  }
  for (const note of plan.notes) console.log(`note: ${note}`);
  if (options.plan) {
    for (const { group, command } of plan.steps) console.log(`[${group}] ${command.join(" ")}`);
    return 0;
  }
  return execute(plan.steps);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
