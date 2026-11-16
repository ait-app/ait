// Run: node scripts/paseo-timeline-fixtures.mjs /path/to/paseo
// Execute the pinned upstream projection tests and capture their inputs/results.
// Rust consumes the resulting JSON without a Node or Paseo dependency.
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
  "packages/server/src/server/agent/timeline-projection.ts",
  "packages/server/src/server/agent/timeline-projection.test.ts",
  "packages/protocol/src/timeline-identity.ts",
  "packages/protocol/src/tool-name-normalization.ts",
  "packages/protocol/src/path-utils.ts",
  "packages/protocol/src/tool-call-display.ts",
  "packages/server/src/server/agent/activity-curator.ts",
  "packages/server/src/server/agent/activity-curator.test.ts",
];
const sources = files.map((file) => fs.readFileSync(path.join(root, file), "utf8"));
const compile = (source) =>
  ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText;
const moduleUrl = (source) =>
  `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const identityUrl = moduleUrl(compile(sources[2]));
const projectionCode = compile(sources[0]).replace(
  '"@getpaseo/protocol/timeline-identity"',
  JSON.stringify(identityUrl),
);
const projectionUrl = moduleUrl(projectionCode);
const upstream = await import(projectionUrl);
const names = [
  "projectTimelineRows",
  "selectProjectedTimelinePage",
  "selectTimelineWindowByProjectedLimit",
];
const cases = [];
let testName = "";
let testCount = 0;
const functions = names.map((name) => (input) => {
  const result = upstream[name](input);
  if (name !== "selectTimelineWindowByProjectedLimit") {
    cases.push(structuredClone({ name: testName, operation: name, input, expected: result }));
  }
  return result;
});
const expect = (actual) => ({
  toBe: (expected) => assert.equal(actual, expected, testName),
  toBeNull: () => assert.equal(actual, null, testName),
  toEqual: (expected) => assert.deepEqual(actual, expected, testName),
  toHaveLength: (expected) => assert.equal(actual.length, expected, testName),
  toContain: (expected) => assert.ok(actual.includes(expected), testName),
  toMatch: (expected) => assert.match(actual, expected, testName),
  toMatchObject: (expected) => {
    for (const [key, value] of Object.entries(expected))
      assert.deepEqual(actual[key], value, testName);
  },
  toThrow: (expected) =>
    assert.throws(actual, (error) => error.message.includes(expected), testName),
  not: { toContain: (expected) => assert.ok(!actual.includes(expected), testName) },
});
const tests = compile(sources[1])
  .replace(/^import[\s\S]*?from .*?;\n/gm, "")
  .replace(/export \{\};?\s*$/, "");
Function(
  "describe",
  "test",
  "expect",
  ...names,
  tests,
)(
  (_name, body) => body(),
  (name, body) => {
    testName = name;
    body();
    testCount += 1;
  },
  expect,
  ...functions,
);
const normalizationUrl = moduleUrl(compile(sources[3]));
const pathUrl = moduleUrl(compile(sources[4]));
const displayUrl = moduleUrl(
  compile(sources[5])
    .replace('"./tool-name-normalization.js"', JSON.stringify(normalizationUrl))
    .replace('"./path-utils.js"', JSON.stringify(pathUrl)),
);
const curator = await import(
  moduleUrl(
    compile(sources[6])
      .replace('"@getpaseo/protocol/tool-name-normalization"', JSON.stringify(normalizationUrl))
      .replace('"@getpaseo/protocol/tool-call-display"', JSON.stringify(displayUrl))
      .replace('"./timeline-projection.js"', JSON.stringify(projectionUrl)),
  )
);
const forkCases = [];
const forkTests = compile(sources[7])
  .replace(/^import[\s\S]*?from .*?;\n/gm, "")
  .replace(/export \{\};?\s*$/, "");
Function(
  "describe",
  "it",
  "expect",
  "buildAgentForkContextAttachment",
  "curateAgentActivity",
  forkTests,
)(
  (_name, body) => body(),
  (name, body) => {
    testName = name;
    body();
    testCount += 1;
  },
  expect,
  (input) => {
    try {
      const expected = curator.buildAgentForkContextAttachment(input);
      forkCases.push(structuredClone({ name: testName, input, expected }));
      return expected;
    } catch (error) {
      forkCases.push(structuredClone({ name: testName, input, error: error.message }));
      throw error;
    }
  },
  curator.curateAgentActivity,
);
const source = {
  revision,
  sourceFiles: Object.fromEntries(
    files.map((file, i) => [file, crypto.createHash("sha256").update(sources[i]).digest("hex")]),
  ),
  upstreamTestsExecuted: testCount,
};
const destination = "crates/provider/runtime/tests/fixtures/paseo-timeline-projection.json";
writeFixture(destination, { source, cases });
console.log(JSON.stringify({ destination, tests: testCount, cases: cases.length }));
const forkDestination = "crates/provider/runtime/tests/fixtures/paseo-fork-context.json";
writeFixture(forkDestination, { source, cases: forkCases });
console.log(JSON.stringify({ destination: forkDestination, cases: forkCases.length }));

function writeFixture(destination, value) {
  const content = `${JSON.stringify(value, null, 2)}\n`;
  if (process.argv.includes("--check")) {
    assert.equal(fs.readFileSync(destination, "utf8"), content, `${destination} drifted`);
  } else {
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, content);
  }
}
