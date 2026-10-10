import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const files = [
    path.join(repository, "README.md"),
    path.join(repository, "AGENTS.md"),
    ...["docs", "apps", "packages", "crates", "bins", "assets/brand", "paseo"].flatMap(
      (directory) => markdownFiles(path.join(repository, directory)),
    ),
  ];
  const problems = checkDocumentationLinks(files);
  if (problems.length) {
    console.error(problems.map((problem) => problem.replaceAll(repository, "")).join("\n"));
    process.exitCode = 1;
  } else {
    console.log(`Verified local links in ${files.length} Markdown documents.`);
  }
}
