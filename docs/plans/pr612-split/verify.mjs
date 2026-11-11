import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const directory = dirname(fileURLToPath(import.meta.url));
const repository = resolve(directory, "../../..");
const load = (name) => JSON.parse(readFileSync(resolve(directory, name), "utf8"));
const git = (...args) =>
  execFileSync("git", args, { cwd: repository, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
const hash = (value) => createHash("sha256").update(value).digest("hex");
const unique = (values, label) => assert.equal(new Set(values).size, values.length, label);
const sorted = (values) => [...values].sort();
const sources = load("sources.json");
const evidence = load("evidence.json");
const sections = load("sections.json");
const commands = load("commands.json");
const paths = load("net-diff.json");
const tasks = load("tasks.json");
const reconciliation = load("main-reconciliation.json");
const taskIds = [
  ...Array.from({ length: 34 }, (_, index) => String(index).padStart(2, "0")),
  "S1",
  "S2",
  "S3",
];
assert.deepEqual(
  tasks.map((task) => task.id),
  taskIds,
  "complete ordered task queue",
);
assert.equal(sources.donor_commit, "fd2798e46aa90a8dea48dd8a15e84028dbb981b4");
assert.equal(sources.main_at_inspection, "248a93cf6ab409c8151e3a45ef6803ad418d6305");
assert.equal(sources.pr.headRefOid, sources.donor_commit);
assert.equal(hash(sources.pr.body), sources.sha256.pr_body);
assert.equal(hash(sources.commands_tsv), sources.sha256.commands_tsv);
assert.equal(git("show", `${sources.donor_commit}:${sources.commands_path}`), sources.commands_tsv);
assert.equal(
  git("rev-parse", `${sources.donor_commit}:${sources.commands_path}`).trim(),
  sources.commands_blob,
);
assert.equal(
  git("merge-base", sources.main_at_inspection, sources.donor_commit).trim(),
  sources.merge_base,
);

const matches = [...sources.pr.body.matchAll(/^## (\d+)\. (.+)$/gm)];
assert.equal(matches.length, 33);
assert.deepEqual(
  sections.map((section) => section.id),
  Array.from({ length: 33 }, (_, index) => index + 1),
);
for (const [index, match] of matches.entries()) {
  const section = sections[index];
  const end = sources.pr.body.indexOf("\n## ", match.index + match[0].length);
  assert.equal(section.title, match[2]);
  assert.equal(section.body, sources.pr.body.slice(match.index, end < 0 ? undefined : end));
}

const [header, ...rows] = sources.commands_tsv
  .trimEnd()
  .split("\n")
  .map((line) => line.split("\t"));
assert.equal(rows.length, 88);
assert.equal(commands.length, 88);
unique(
  commands.map((command) => command.command),
  "unique command records",
);
for (const [index, row] of rows.entries()) {
  const command = commands[index];
  assert.equal(row.length, header.length, `TSV columns at row ${index + 2}`);
  assert.deepEqual(
    command.source,
    Object.fromEntries(header.map((name, column) => [name, row[column]])),
  );
  assert.equal(command.id, `C${String(index + 1).padStart(2, "0")}`);
  assert.equal(command.source_row, index + 2);
  assert.equal(command.command, command.source["命令"]);
  assert.deepEqual(
    command.aliases,
    command.source["别名"] ? command.source["别名"].split(/\s+/) : [],
  );
}

const numstat = git("diff", "--numstat", sources.merge_base, sources.donor_commit)
  .trimEnd()
  .split("\n")
  .map((line) => {
    const [added, deleted, path] = line.split("\t");
    return { added, deleted, path };
  });
assert.equal(paths.length, 547);
assert.deepEqual(
  paths.map(({ added, deleted, path }) => ({ added, deleted, path })),
  numstat,
);
unique(
  paths.map((path) => path.path),
  "every donor path exactly once",
);
const totals = paths.reduce(
  (sum, path) => ({
    added: sum.added + (path.added === "-" ? 0 : Number(path.added)),
    deleted: sum.deleted + (path.deleted === "-" ? 0 : Number(path.deleted)),
  }),
  { added: 0, deleted: 0 },
);
assert.deepEqual(totals, { added: 41826, deleted: 1125 });
const mainPaths = new Set(
  git("diff", "--name-only", sources.merge_base, sources.main_at_inspection).trimEnd().split("\n"),
);
for (const path of paths) {
  assert.equal(path.also_changed_on_main, mainPaths.has(path.path), path.path);
  assert.deepEqual(
    path.source_sections,
    sections.filter((section) => section.paths.includes(path.path)).map((section) => section.id),
  );
}
assert.equal(paths.filter((path) => path.also_changed_on_main).length, 26);

const dispositions = new Set(["rewrite", "migrate", "reference", "exclude", "defer"]);
for (const record of [...sections, ...commands, ...paths]) {
  assert.ok(dispositions.has(record.disposition), "explicit disposition");
  assert.ok(record.decision.length > 0, "explicit decision");
  assert.ok(record.tasks.length > 0, "destination task or review owner");
  unique(record.tasks, "unique destination tasks");
  for (const id of record.tasks) assert.ok(taskIds.includes(id), `known task ${id}`);
}
const required = [
  "id",
  "priority",
  "depends_on",
  "donor_sections",
  "donor_paths",
  "implementation",
  "acceptance",
  "branch",
  "base",
  "commit",
  "pr_url",
  "checks",
  "blockers",
];
for (const [index, task] of tasks.entries()) {
  for (const field of required) assert.ok(Object.hasOwn(task, field), `${task.id}.${field}`);
  assert.equal(task.priority, index);
  assert.ok(task.implementation.length > 0 && task.acceptance.length > 0);
  assert.ok(Array.isArray(task.blockers));
  assert.ok(Array.isArray(task.checks.required) && Array.isArray(task.checks.results));
  unique(task.depends_on, `${task.id} unique dependencies`);
  if (task.id !== "00") assert.ok(task.depends_on.includes("00"));
  for (const dependency of task.depends_on)
    assert.ok(taskIds.includes(dependency) && dependency !== task.id);
  assert.deepEqual(
    task.donor_sections,
    sections.filter((section) => section.tasks.includes(task.id)).map((section) => section.id),
  );
  assert.deepEqual(
    sorted(task.donor_paths),
    sorted(paths.filter((path) => path.tasks.includes(task.id)).map((path) => path.path)),
  );
  if (task.status === "delivered") {
    for (const field of ["branch", "base", "commit", "pr_url"])
      assert.ok(task[field], `delivered ${task.id}.${field}`);
    assert.equal(task.blockers.length, 0, `delivered ${task.id} has no blockers`);
    assert.ok(task.checks.results.length > 0, `delivered ${task.id} has checks`);
  }
}
const visited = new Set();
function visit(id, stack = new Set()) {
  assert.ok(!stack.has(id), `dependency cycle at ${id}`);
  if (visited.has(id)) return;
  const next = new Set(stack).add(id);
  for (const dependency of tasks.find((task) => task.id === id).depends_on) visit(dependency, next);
  visited.add(id);
}
for (const id of taskIds) visit(id);
assert.equal(evidence.worktree.base, sources.main_at_inspection);
assert.equal(evidence.donor.head, sources.donor_commit);
assert.equal(evidence.comparison.merge_base, sources.merge_base);
assert.equal(tasks.find((task) => task.id === "13").status, "blocked");
assert.ok(
  paths
    .filter((path) => path.path.startsWith("docs/assets/"))
    .every((path) => path.disposition === "reference"),
);
assert.equal(reconciliation.main_commit, sources.main_at_inspection);
assert.deepEqual(
  reconciliation.overlaps.map((entry) => entry.path),
  paths.filter((path) => path.also_changed_on_main).map((path) => path.path),
);
for (const entry of reconciliation.overlaps) {
  assert.equal(
    git("rev-parse", `${sources.main_at_inspection}:${entry.path}`).trim(),
    entry.main_blob,
  );
  assert.equal(git("rev-parse", `${sources.merge_base}:${entry.path}`).trim(), entry.base_blob);
  assert.equal(entry.whole_file_main_covered, false);
  assert.ok(entry.decision.length > 0);
  const hunks = git(
    "diff",
    "--unified=0",
    sources.merge_base,
    sources.main_at_inspection,
    "--",
    entry.path,
  )
    .split("\n")
    .filter((line) => line.startsWith("@@"));
  assert.deepEqual(entry.main_hunks, hunks);
}
assert.equal(reconciliation.main_covered_behaviors.length, 4);
assert.equal(reconciliation.deletion_decisions.length, 4);
for (const item of [
  ...reconciliation.main_covered_behaviors,
  ...reconciliation.deletion_decisions,
]) {
  assert.ok(["main-covered", "delete"].includes(item.disposition));
  for (const task of item.tasks) assert.ok(taskIds.includes(task));
  assert.ok(item.evidence.length > 0);
  for (const source of item.evidence) {
    assert.ok([sources.main_at_inspection, sources.donor_commit].includes(source.commit));
    assert.equal(git("rev-parse", `${source.commit}:${source.path}`).trim(), source.blob);
    const lines = git("show", `${source.commit}:${source.path}`).split("\n");
    assert.equal(lines.slice(source.lines[0] - 1, source.lines[1]).join("\n"), source.excerpt);
  }
}
console.log(
  "PR612 inventory valid: 33 sections, 88 commands, 547 paths (+41826/-1125; 26 individually reconciled main overlaps), 37 tasks, 4 main-covered behaviors and 4 deletion decisions; source hashes, excerpts and mappings match.",
);
