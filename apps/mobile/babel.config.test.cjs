// Behavior test for the React Compiler sources filter in babel.config.js.
// Runs the real babel-preset-expo + babel-plugin-react-compiler pipeline with the same
// caller contract Expo/Metro uses when app.config.js enables experiments.reactCompiler.
const assert = require("node:assert/strict");
const path = require("node:path");
const test = require("node:test");
const babel = require("@babel/core");

const repoRoot = path.resolve(__dirname, "../..");
const configFile = path.join(__dirname, "babel.config.js");
const caller = {
  name: "metro",
  platform: "web",
  supportsReactCompiler: true,
  isDev: true,
};

const COMPONENT_SOURCE = `
import { useState } from "react";
export function Counter({ label }) {
  const [count, setCount] = useState(0);
  return (
    <button onClick={() => setCount(count + 1)}>
      {label}: {count}
    </button>
  );
}
`;

// Hook-bearing source: the compiler memoizes it unless the sources filter excludes the file.
// Protocol schemas themselves are plain functions the compiler skips; this fixture proves the
// filter, not the compiler's component heuristic, keeps protocol files out of compilation.
const HOOK_SOURCE = `
import { useState } from "react";
export function SchemaPreview({ label }) {
  const [count, setCount] = useState(0);
  return label + ":" + count + ":" + setCount;
}
`;

function transform(filename, code) {
  return babel.transformSync(code, {
    filename,
    configFile,
    babelrc: false,
    caller,
    sourceType: "module",
  }).code;
}

test("application components are still transformed by the React Compiler", () => {
  const code = transform(
    path.join(repoRoot, "apps/mobile/src/components/counter.tsx"),
    COMPONENT_SOURCE,
  );
  assert.match(code, /react\/compiler-runtime/);
});

test("protocol schemas are excluded on POSIX paths", () => {
  const code = transform(path.join(repoRoot, "packages/protocol/src/agent-labels.ts"), HOOK_SOURCE);
  assert.doesNotMatch(code, /react\/compiler-runtime/);
});

test("protocol schemas are excluded on Windows paths", () => {
  const code = transform(
    "C:\\Users\\dev\\ait\\packages\\protocol\\src\\agent-labels.ts",
    HOOK_SOURCE,
  );
  assert.doesNotMatch(code, /react\/compiler-runtime/);
});

test("mixed separators still exclude protocol sources", () => {
  const code = transform(
    "C:/Users/dev/ait\\packages\\protocol/src/generated/validation/ws-outbound.aot.ts",
    HOOK_SOURCE,
  );
  assert.doesNotMatch(code, /react\/compiler-runtime/);
});

test("lookalike paths outside packages/protocol stay compiled", () => {
  const code = transform(
    path.join(repoRoot, "apps/mobile/src/packages/protocol-bridge.tsx"),
    COMPONENT_SOURCE,
  );
  assert.match(code, /react\/compiler-runtime/);
});
