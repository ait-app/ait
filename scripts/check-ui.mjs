import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
  return result.stdout;
};
const base = process.argv[2];
const files = new Set(
  (base && !/^0+$/.test(base)
    ? run("git", ["diff", "--name-only", "--diff-filter=ACMR", "-z", base, "HEAD"])
    : base
      ? run("git", ["ls-files", "-z"])
      : run("git", ["diff", "--name-only", "--diff-filter=ACMR", "-z", "HEAD"]) +
        run("git", ["ls-files", "--others", "--exclude-standard", "-z"])
  )
    .split("\0")
    .filter(Boolean),
);
const lintFiles = [...files].filter(
  (file) =>
    /^(?:apps\/|packages\/|scripts\/|\.github\/|[^/]+$)/.test(file) &&
    /\.[cm]?[jt]sx?$/.test(file) &&
    !/(?:^|\/)(?:generated\/|[^/]+\.gen\.[jt]sx?$)/.test(file) &&
    existsSync(new URL(`../${file}`, import.meta.url)),
);
if (lintFiles.length > 0)
  run("npm", ["exec", "--", "oxlint", "--deny-warnings", ...lintFiles], { stdio: "inherit" });
console.log(`Checked lint for ${lintFiles.length} changed UI/config files.`);
