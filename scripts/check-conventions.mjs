// Repository conventions a text scan can enforce: AGENTS.md ("Tests wait for events") and the
// test-module and file-size rules in docs/policy/rust.md.
//
//   node scripts/check-conventions.mjs           check the working tree
//   node scripts/check-conventions.mjs --update  lower the real-time wait baseline after a cleanup
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repository = fileURLToPath(new URL("../", import.meta.url));
export const BASELINE_FILE = "scripts/real-time-waits.json";
// Its fixtures spell out the patterns on purpose.
const SELF_TEST = "scripts/check-conventions.test.mjs";
const MAX_RUST_LINES = 5000;

const RUST_TEST_PATH = /(?:^|\/)(?:tests|testing|test_support)(?:\/|\.rs$)|_tests?\.rs$/;
// Test modules proper; `testing` and `test_support` helpers may group items in inline modules.
const RUST_TEST_MODULE_PATH = /(?:^|\/)tests(?:\/|\.rs$)/;
const SCRIPT_TEST_PATH = /\.(?:test|spec)\.[cm]?[jt]sx?$/;
// Calls that suspend for wall-clock time. `fn sleep(` definitions and `time.sleep` inside embedded
// Python fixtures are not waits by the test itself.
const RUST_WAIT =
  /(?<![\w.]|\bfn\s+)(?:(?:tokio::time|std::thread|thread|time)::)?sleep(?:_until)?\s*\(/g;
const SCRIPT_WAIT =
  /\bwaitForTimeout\s*\(|\bsetTimeout\s*\(\s*(?:resolve|res|r|done|next)\b|\bsetTimeout\s*\(\s*\(\)\s*=>\s*(?:resolve|res|r)\s*\(|\bawait\s+(?:sleep|delay|wait|setTimeout)\s*\(/g;
// A wait on a paused Tokio clock or Vitest fake timers advances without wall-clock time.
const VIRTUAL_TIME = /\/\/\s*virtual time\b/;
const COMMENT = /^\s*(?:\/\/|\/\*|\*)/;

/** Which wait pattern applies to a tracked file, or null when it is not a test file. */
export function waitPattern(file) {
  if (file === SELF_TEST) return null;
  if (file.endsWith(".rs")) return RUST_TEST_PATH.test(file) ? RUST_WAIT : null;
  return SCRIPT_TEST_PATH.test(file) ? SCRIPT_WAIT : null;
}

/** Counts real-time waits in one test file, skipping comments and `// virtual time` lines. */
export function countRealTimeWaits(text, pattern) {
  let count = 0;
  for (const line of text.split("\n")) {
    if (COMMENT.test(line) || VIRTUAL_TIME.test(line)) continue;
    count += line.match(pattern)?.length ?? 0;
  }
  return count;
}

/**
 * Compares counts with the committed baseline. A rise or a new file is a regression; a drop must
 * be recorded with `--update` so the next regression is measured from the lower count.
 */
export function compareWaits(baseline, counts) {
  const regressions = [];
  const improvements = [];
  for (const file of new Set([...Object.keys(baseline), ...Object.keys(counts)])) {
    const allowed = baseline[file] ?? 0;
    const actual = counts[file] ?? 0;
    if (actual > allowed) {
      regressions.push(
        `${file}: ${actual} real-time waits in tests, baseline ${allowed}; await the event, ` +
          "channel or process exit instead (AGENTS.md)",
      );
    } else if (actual < allowed) {
      improvements.push(
        `${file}: ${actual} real-time waits in tests, baseline ${allowed}; ` +
          `run node scripts/check-conventions.mjs --update to lower ${BASELINE_FILE}`,
      );
    }
  }
  return { regressions: regressions.toSorted(), improvements: improvements.toSorted() };
}

/** The baseline after a cleanup: each count only goes down, and files at zero drop out. */
export function loweredBaseline(baseline, counts) {
  return Object.fromEntries(
    Object.entries(baseline)
      .map(([file, allowed]) => [file, Math.min(allowed, counts[file] ?? 0)])
      .filter(([, allowed]) => allowed > 0)
      .toSorted(([left], [right]) => (left < right ? -1 : 1)),
  );
}

const INLINE_TEST_MODULE =
  /#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{|^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+tests?\s*\{/m;
const ANY_INLINE_MODULE = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/m;

/** Rust layout rules from docs/policy/rust.md for one source file. */
export function checkRustLayout(file, text) {
  const problems = [];
  const lines = text.endsWith("\n") ? text.split("\n").length - 1 : text.split("\n").length;
  if (lines > MAX_RUST_LINES) {
    problems.push(
      `${file}: ${lines} lines exceeds ${MAX_RUST_LINES}; split it into a module directory`,
    );
  }
  const inline = RUST_TEST_MODULE_PATH.test(file) ? ANY_INLINE_MODULE : INLINE_TEST_MODULE;
  if (inline.test(text)) {
    problems.push(
      `${file}: inline test module body; declare it with \`mod name;\` and move it to a child file`,
    );
  }
  return problems;
}

function trackedFiles() {
  const result = spawnSync(
    "git",
    ["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
    {
      cwd: repository,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  if (result.status !== 0) throw new Error(`git ls-files failed: ${result.stderr.trim()}`);
  return [...new Set(result.stdout.split("\0"))].filter(
    (file) => file && existsSync(path.join(repository, file)),
  );
}

function scan(files) {
  const counts = {};
  const problems = [];
  for (const file of files) {
    const pattern = waitPattern(file);
    const rust = file.endsWith(".rs");
    if (!pattern && !rust) continue;
    const text = readFileSync(path.join(repository, file), "utf8");
    if (rust) problems.push(...checkRustLayout(file, text));
    const count = pattern ? countRealTimeWaits(text, pattern) : 0;
    if (count > 0) counts[file] = count;
  }
  return { counts, problems };
}

function main(argv) {
  const update = argv.includes("--update");
  const baselinePath = path.join(repository, BASELINE_FILE);
  const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
  const files = trackedFiles();
  const { counts, problems } = scan(files);
  const { regressions, improvements } = compareWaits(baseline, counts);
  if (update && improvements.length > 0) {
    writeFileSync(baselinePath, `${JSON.stringify(loweredBaseline(baseline, counts), null, 2)}\n`);
    console.log(`Lowered ${improvements.length} entries in ${BASELINE_FILE}.`);
  }
  const failures = [...problems, ...regressions, ...(update ? [] : improvements)];
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    return 1;
  }
  const waits = Object.values(counts).reduce((sum, count) => sum + count, 0);
  console.log(
    `Checked ${files.length} files: Rust layout holds; ${waits} real-time waits in ` +
      `${Object.keys(counts).length} test files match ${BASELINE_FILE}.`,
  );
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = main(process.argv.slice(2));
}
