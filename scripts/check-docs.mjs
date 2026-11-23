import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { DECISIONS_DIRECTORY, checkDecisions, readDecisions } from "./adr.mjs";

const repository = fileURLToPath(new URL("../", import.meta.url));
const excludedDirectories = new Set([
  "node_modules",
  "dist",
  "build",
  "release",
  "release-resources",
  "test-results",
]);

function markdownFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (entry.name.startsWith(".") || excludedDirectories.has(entry.name)) return [];
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) return markdownFiles(file);
    return entry.isFile() && entry.name.endsWith(".md") ? [file] : [];
  });
}

export function checkDocumentationLinks(files) {
  const problems = [];
  for (const file of files) {
    const markdown = readFileSync(file, "utf8").replace(/^```[^\n]*\n[\s\S]*?^```/gm, "");
    for (const match of markdown.matchAll(/\[[^\]]*\]\((<[^>]*>|[^\s)]+)(?:\s+"[^"]*")?\)/g)) {
      const target = match[1].replace(/^<|>$/g, "");
      if (/^(?:[a-z][a-z\d+.-]*:|#|\/\/)/i.test(target)) continue;
      const localPath = decodeURIComponent(target.split(/[?#]/, 1)[0]);
      if (!localPath || existsSync(path.resolve(path.dirname(file), localPath))) continue;
      problems.push(`${file}: missing local link ${target}`);
    }
  }
  return problems;
}

/**
 * docs/policy/documentation.md: `docs/README.md` navigates by task and leaves ADRs to the generated
 * index, and per-PR evidence goes in the PR description rather than `docs/reports/`.
 */
export function checkDocumentationLayout({ navigation, reports }) {
  const problems = [];
  for (const match of navigation.matchAll(/\]\((decisions\/[^/)\s]+\/adr-[^)\s]+)\)/g)) {
    problems.push(
      `docs/README.md: links ADR ${match[1]}; link the spec or operations page, or decisions/README.md`,
    );
  }
  for (const entry of reports.filter((name) => name !== "README.md").toSorted()) {
    problems.push(`docs/reports/${entry}: put verification evidence in the PR description`);
  }
  return problems;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const files = [
    path.join(repository, "README.md"),
    path.join(repository, "AGENTS.md"),
    ...["docs", "apps", "packages", "crates", "bins", "assets/brand", "paseo"].flatMap(
      (directory) => markdownFiles(path.join(repository, directory)),
    ),
  ];
  const decisions = readDecisions(repository);
  const problems = [
    ...checkDocumentationLinks(files),
    ...checkDocumentationLayout({
      navigation: readFileSync(path.join(repository, "docs", "README.md"), "utf8"),
      reports: readdirSync(path.join(repository, "docs", "reports")),
    }),
    ...checkDecisions(decisions, {
      exists: (owned) => existsSync(path.join(repository, owned)),
      index: readFileSync(path.join(repository, DECISIONS_DIRECTORY, "README.md"), "utf8"),
    }),
  ];
  if (problems.length) {
    console.error(problems.map((problem) => problem.replaceAll(repository, "")).join("\n"));
    process.exitCode = 1;
  } else {
    console.log(
      `Verified local links in ${files.length} Markdown documents and metadata of ${decisions.length} ADRs.`,
    );
  }
}
