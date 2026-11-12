// Run: node scripts/paseo-history-fixtures.mjs /path/to/paseo
// Execute upstream history-search tests and record bounded-distance oracle cases.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";

const root = path.resolve(process.argv[2] ?? "../paseo");
const require = createRequire(path.join(root, "package.json"));
const ts = require("typescript");
const revision = execFileSync("git", ["-C", root, "rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
assert.equal(revision, "30178c4f58b67f8472901356e1484022bd835de0");
const files = [
  "packages/protocol/src/search/text-match.ts",
  "packages/server/src/server/agent-history-search.ts",
  "packages/server/src/server/agent-history-search.test.ts",
];
const sources = files.map((file) => fs.readFileSync(path.join(root, file), "utf8"));
const compile = (source) =>
  ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText;
const moduleUrl = (source) =>
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const matchUrl = moduleUrl(`${compile(sources[0])}\nexport { boundedEditDistance };`);
const match = await import(matchUrl);
const history = await import(
  moduleUrl(
    compile(sources[1]).replace('"@getpaseo/protocol/search/text-match"', JSON.stringify(matchUrl)),
  )
);
let name = "";
let tests = 0;
const cases = [];
const code = compile(sources[2])
  .replace(/^import[\s\S]*?from .*?;\n/gm, "")
  .replace(/export \{\};?\s*$/, "");
Function(
  "describe",
  "it",
  "expect",
  "matchesAgentHistoryQuery",
  code,
)(
  (_name, body) => body(),
  (next, body) => {
    name = next;
    body();
    tests += 1;
  },
  (actual) => ({ toBe: (expected) => assert.equal(actual, expected, name) }),
  (query, candidate) => {
    const expected = history.matchesAgentHistoryQuery(query, candidate);
    const fields = [
      candidate.project.workspaceName ?? "",
      candidate.agent.title ?? "",
      candidate.project.checkout.currentBranch ?? "",
      candidate.project.projectName,
    ];
    cases.push({ name, query, fields, expected });
    return expected;
  },
);
const distances = [];
let seed = 0x5eed;
const random = (length) => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return seed % length;
};
for (let index = 0; index < 160; index += 1) {
  const query = [
    "terminal",
    "configuration",
    "billing",
    "typescript",
    "abcdef",
    "regression",
    "workspace",
  ][random(7)];
  let word = query;
  for (let edits = 0; edits < index % 5; edits += 1) {
    const at = random(word.length);
    switch (random(4)) {
      case 0:
        word = word.slice(0, at) + word.slice(at + 1);
        break;
      case 1:
        word = word.slice(0, at) + "x" + word.slice(at);
        break;
      case 2:
        word = word.slice(0, at) + "z" + word.slice(at + 1);
        break;
      case 3:
        if (at + 1 < word.length)
          word = word.slice(0, at) + word[at + 1] + word[at] + word.slice(at + 2);
        break;
    }
  }
  const budget = 1 + (index % 2);
  distances.push({
    query,
    word,
    budget,
    expected: match.boundedEditDistance(query, word, budget) !== null,
  });
}
const source = {
  revision,
  upstreamTestsExecuted: tests,
  sourceFiles: Object.fromEntries(
    files.map((file, index) => [
      file,
      crypto.createHash("sha256").update(sources[index]).digest("hex"),
    ]),
  ),
};
const destination = "crates/provider/runtime/tests/fixtures/paseo-history-search.json";
const content = `${JSON.stringify({ source, cases, distances }, null, 2)}\n`;
if (process.argv.includes("--check")) {
  assert.equal(fs.readFileSync(destination, "utf8"), content, `${destination} drifted`);
} else {
  fs.writeFileSync(destination, content);
}
console.log(
  JSON.stringify({ destination, tests, cases: cases.length, distances: distances.length }),
);
