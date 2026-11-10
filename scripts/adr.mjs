// ADR metadata: reads each decision's front matter, validates it, renders the category index
// and answers which decisions govern a path.
//
//   node scripts/adr.mjs index            regenerate docs/decisions/README.md
//   node scripts/adr.mjs index --check    fail when the index is out of date
//   node scripts/adr.mjs governs <path>…  list the ADRs in force for files or directories
//                                         (--all also lists superseded and withdrawn ones)
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const repository = fileURLToPath(new URL("../", import.meta.url));
export const DECISIONS_DIRECTORY = "docs/decisions";

export const STATUSES = ["proposed", "accepted", "superseded", "withdrawn"];
const REQUIRED_FIELDS = ["id", "status", "date", "paths"];
const LIST_FIELDS = new Set(["supersedes", "amends", "paths"]);
const KNOWN_FIELDS = new Set([...REQUIRED_FIELDS, "supersedes", "amends"]);

// Each of these numbers was given to two ADRs before the check existed. They keep their paths;
// any other repeated number, or a third copy of one of these, fails the check.
export const LEGACY_DUPLICATE_NUMBERS = new Set([
  "074",
  "082",
  "083",
  "084",
  "087",
  "122",
  "132",
  "140",
  "156",
  "175",
  "177",
  "180",
  "211",
  "215",
  "219",
  "223",
]);

// Index sections, in the order they appear in docs/decisions/README.md.
export const CATEGORIES = [
  ["bot", "Bot"],
  ["daemon", "Daemon 与协议"],
  ["workspace", "工作区、文件与 Git"],
  ["providers", "Provider、Agent 与会话"],
  ["clients", "客户端、连接与品牌"],
  ["branding", "品牌与视觉"],
];

const FILE_NAME = /^adr-(\d{3,})-[a-z0-9-]+\.md$/;

function unquote(value) {
  const trimmed = value.trim();
  return /^(["']).*\1$/.test(trimmed) ? trimmed.slice(1, -1) : trimmed;
}

/** Parses the small YAML subset ADR front matter uses: scalars and lists of plain strings. */
export function parseFrontMatter(text) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/.exec(text);
  if (!match) return null;
  const fields = {};
  const problems = [];
  let key = null;
  const raw = {};
  for (const line of match[1].split(/\r?\n/)) {
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    const entry = /^([A-Za-z_][\w-]*):(.*)$/.exec(line);
    if (entry && !/^\s/.test(line)) {
      key = entry[1];
      if (key in raw) problems.push(`repeats field ${key}`);
      raw[key] = [entry[2].trim()].filter(Boolean);
    } else if (key && /^\s/.test(line)) {
      raw[key].push(line.trim());
    } else {
      problems.push(`cannot parse front matter line "${line}"`);
    }
  }
  for (const [name, parts] of Object.entries(raw)) {
    const joined = parts.join(" ");
    if (joined.startsWith("[")) {
      if (!joined.endsWith("]")) problems.push(`field ${name} has an unterminated list`);
      fields[name] = joined.slice(1, -1).split(",").map(unquote).filter(Boolean);
    } else if (parts.length > 0 && parts.every((part) => part.startsWith("- "))) {
      fields[name] = parts.map((part) => unquote(part.slice(2)));
    } else if (parts.some((part) => part.startsWith("- "))) {
      problems.push(`field ${name} mixes a value with list items`);
      fields[name] = parts.map((part) => unquote(part.replace(/^- /, "")));
    } else {
      fields[name] = unquote(joined);
    }
  }
  return { fields, problems, body: text.slice(match[0].length) };
}

/** Loads every ADR under the decisions directory, sorted by category and newest first. */
export function readDecisions(root = repository) {
  const directory = path.join(root, DECISIONS_DIRECTORY);
  const decisions = [];
  for (const category of readdirSync(directory, { withFileTypes: true })) {
    if (!category.isDirectory()) continue;
    for (const entry of readdirSync(path.join(directory, category.name), { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.startsWith("adr-") || !entry.name.endsWith(".md"))
        continue;
      const file = `${category.name}/${entry.name}`;
      const text = readFileSync(path.join(directory, file), "utf8");
      const frontMatter = parseFrontMatter(text);
      const body = frontMatter ? frontMatter.body : text;
      decisions.push({
        file,
        category: category.name,
        number: FILE_NAME.exec(entry.name)?.[1] ?? null,
        title: /^#\s+(.+?)\s*$/m.exec(body)?.[1] ?? null,
        meta: frontMatter?.fields ?? null,
        parseProblems: frontMatter?.problems ?? [],
      });
    }
  }
  return decisions.toSorted(
    (left, right) =>
      Number(right.number ?? 0) - Number(left.number ?? 0) || left.file.localeCompare(right.file),
  );
}

const asList = (value) => (Array.isArray(value) ? value : value ? [value] : []);

/** Maps each ADR file to the ADRs that name it in `supersedes` or `amends`. */
function reverseRelations(decisions) {
  const supersededBy = new Map();
  const amendedBy = new Map();
  for (const decision of decisions) {
    for (const [field, map] of [
      ["supersedes", supersededBy],
      ["amends", amendedBy],
    ]) {
      for (const target of asList(decision.meta?.[field])) {
        if (!map.has(target)) map.set(target, []);
        map.get(target).push(decision);
      }
    }
  }
  return { supersededBy, amendedBy };
}

function isValidDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(value);
}

/**
 * Returns one message per problem. `exists` resolves repository-relative owning paths;
 * `index` is the current text of the generated index, or null to skip that comparison.
 */
export function checkDecisions(decisions, { exists, index = null } = {}) {
  const problems = [];
  const files = new Set(decisions.map((decision) => decision.file));
  const categories = new Set(CATEGORIES.map(([name]) => name));
  const { supersededBy } = reverseRelations(decisions);
  const report = (decision, message) =>
    problems.push(`${DECISIONS_DIRECTORY}/${decision.file}: ${message}`);

  for (const decision of decisions) {
    if (!decision.number) report(decision, "file name must be adr-<number>-<slug>.md");
    if (!categories.has(decision.category)) {
      report(decision, `unknown category ${decision.category}; add it to CATEGORIES`);
    }
    if (!decision.title?.startsWith(`ADR-${decision.number}`)) {
      report(decision, `title must start with ADR-${decision.number}`);
    }
    const meta = decision.meta;
    if (!meta) {
      report(decision, "missing front matter");
      continue;
    }
    for (const problem of decision.parseProblems) report(decision, problem);
    for (const field of Object.keys(meta)) {
      if (!KNOWN_FIELDS.has(field)) report(decision, `unknown field ${field}`);
      else if (LIST_FIELDS.has(field) && !Array.isArray(meta[field])) {
        report(decision, `field ${field} must be a list`);
      }
    }
    for (const field of REQUIRED_FIELDS) {
      const value = meta[field];
      if (value === undefined || value === "" || (Array.isArray(value) && value.length === 0)) {
        report(decision, `missing required field ${field}`);
      }
    }
    if (meta.id && meta.id !== `ADR-${decision.number}`) {
      report(decision, `id ${meta.id} does not match file number ${decision.number}`);
    }
    if (meta.status && !STATUSES.includes(meta.status)) {
      report(decision, `status ${meta.status} is not one of ${STATUSES.join(", ")}`);
    }
    if (meta.date && !isValidDate(meta.date))
      report(decision, `date ${meta.date} is not YYYY-MM-DD`);
    for (const owned of asList(meta.paths)) {
      if (path.isAbsolute(owned) || owned.split("/").includes("..")) {
        report(decision, `path ${owned} must be relative to the repository root`);
      } else if (exists && !exists(owned.replace(/\/$/, ""))) {
        report(decision, `path ${owned} does not exist`);
      }
    }
    for (const field of ["supersedes", "amends"]) {
      for (const target of asList(meta[field])) {
        if (target === decision.file) report(decision, `${field} names itself`);
        else if (!files.has(target)) report(decision, `${field} target ${target} does not exist`);
      }
    }
    if (meta.status === "superseded" && !supersededBy.has(decision.file)) {
      report(decision, "status is superseded but no ADR lists it in supersedes");
    }
    if (supersededBy.has(decision.file) && meta.status && meta.status !== "superseded") {
      const by = supersededBy.get(decision.file).map((other) => other.file);
      report(decision, `superseded by ${by.join(", ")} but status is ${meta.status}`);
    }
  }

  const byNumber = Map.groupBy(
    decisions.filter((decision) => decision.number),
    (decision) => decision.number,
  );
  for (const [number, copies] of byNumber) {
    if (copies.length < 2) continue;
    if (LEGACY_DUPLICATE_NUMBERS.has(number) && copies.length === 2) continue;
    problems.push(
      `${DECISIONS_DIRECTORY}: ADR-${number} is used by ${copies.length} files ` +
        `(${copies.map((copy) => copy.file).join(", ")}); pick an unused number`,
    );
  }

  if (index !== null && index !== renderIndex(decisions, index)) {
    problems.push(
      `${DECISIONS_DIRECTORY}/README.md: out of date; run node scripts/adr.mjs index to regenerate it`,
    );
  }
  return problems;
}

const reference = (decision) => `[ADR-${decision.number}](${decision.file})`;

function annotation(decision, relations) {
  const status = decision.meta?.status;
  const superseders = relations.supersededBy.get(decision.file) ?? [];
  const amenders = (relations.amendedBy.get(decision.file) ?? []).toSorted(
    (left, right) => Number(left.number) - Number(right.number),
  );
  const notes = [];
  if (status === "superseded") {
    notes.push(
      superseders.length ? `已由 ${superseders.map(reference).join("、")} 取代` : "已取代",
    );
  } else if (status === "withdrawn") notes.push("已撤销");
  else if (status === "proposed") notes.push("提议中");
  if (status !== "superseded" && amenders.length) {
    notes.push(`经 ${amenders.map(reference).join("、")} 修订`);
  }
  return notes.length ? `（${notes.join("；")}）` : "";
}

/** Keeps the hand-written introduction (everything before the first section) of `current`. */
export function renderIndex(decisions, current) {
  const sectionStart = current.search(/^## /m);
  const intro = (sectionStart === -1 ? current : current.slice(0, sectionStart)).trimEnd();
  const relations = reverseRelations(decisions);
  const sections = CATEGORIES.map(([category, label]) => {
    const lines = decisions
      .filter((decision) => decision.category === category)
      .map(
        (decision) =>
          `- [${decision.title ?? decision.file}](${decision.file})${annotation(decision, relations)}`,
      );
    return `## ${label}\n\n${lines.join("\n")}\n`;
  });
  return `${intro}\n\n${sections.join("\n")}`;
}

/** Normalises a user-supplied path to a repository-relative POSIX path. */
export function repositoryPath(target, { cwd = process.cwd(), root = repository } = {}) {
  const relative = path.relative(root, path.resolve(cwd, target)).split(path.sep).join("/");
  return relative.replace(/\/$/, "");
}

/** ADRs whose owning paths contain `target`, or lie inside it when `target` is a directory. */
export function governing(decisions, target, { all = false } = {}) {
  const relations = reverseRelations(decisions);
  return decisions
    .filter((decision) => all || decision.meta?.status === "accepted")
    .filter((decision) =>
      asList(decision.meta?.paths).some((owned) => {
        const prefix = owned.replace(/\/$/, "");
        return (
          target === "" ||
          prefix === target ||
          target.startsWith(`${prefix}/`) ||
          prefix.startsWith(`${target}/`)
        );
      }),
    )
    .map((decision) => ({
      ...decision,
      amendedBy: (relations.amendedBy.get(decision.file) ?? [])
        .toSorted((left, right) => Number(left.number) - Number(right.number))
        .map((other) => `ADR-${other.number}`),
    }));
}

function main(argv) {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      check: { type: "boolean", default: false },
      all: { type: "boolean", default: false },
    },
  });
  const [command, ...targets] = positionals;
  const decisions = readDecisions();
  const indexFile = path.join(repository, DECISIONS_DIRECTORY, "README.md");

  if (command === "index") {
    const current = readFileSync(indexFile, "utf8");
    const rendered = renderIndex(decisions, current);
    if (values.check) {
      if (rendered === current) return 0;
      console.error(
        `${DECISIONS_DIRECTORY}/README.md is out of date; run node scripts/adr.mjs index`,
      );
      return 1;
    }
    if (rendered !== current) writeFileSync(indexFile, rendered);
    console.log(`Indexed ${decisions.length} ADRs in ${DECISIONS_DIRECTORY}/README.md.`);
    return 0;
  }
  if (command === "governs" && targets.length > 0) {
    for (const target of targets) {
      const relative = repositoryPath(target);
      const matches = governing(decisions, relative, { all: values.all });
      console.log(`${relative || "."}: ${matches.length} ADR${matches.length === 1 ? "" : "s"}`);
      for (const decision of matches) {
        const amended = decision.amendedBy.length
          ? ` [amended by ${decision.amendedBy.join(", ")}]`
          : "";
        console.log(
          `  ${decision.meta.status.padEnd(10)} ${DECISIONS_DIRECTORY}/${decision.file}  ${decision.title}${amended}`,
        );
      }
    }
    return 0;
  }
  console.error(
    "usage: node scripts/adr.mjs index [--check] | node scripts/adr.mjs governs [--all] <path>…",
  );
  return 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
