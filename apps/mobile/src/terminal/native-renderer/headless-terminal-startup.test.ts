import { buildSync } from "esbuild";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createContext, runInContext } from "node:vm";
import { describe, expect, test } from "vitest";

const dependencyDirectory = dirname(createRequire(import.meta.url).resolve("@xterm/headless"));

describe.each(["js", "mjs"])("headless terminal startup (%s)", (extension) => {
  // Load the real adapter and its dependency in a fresh runtime. Importing them in
  // Vitest's Node process hides startup failures caused by React Native's globals.
  const bundle = buildSync({
    entryPoints: [fileURLToPath(new URL("./headless-terminal-state.ts", import.meta.url))],
    bundle: true,
    write: false,
    platform: "neutral",
    mainFields: ["main"],
    format: "cjs",
    alias: {
      "@xterm/headless": join(dependencyDirectory, `xterm-headless.${extension}`),
    },
  }).outputFiles[0].text;

  test.each([
    {
      name: "React Native",
      globals: { navigator: { product: "ReactNative" }, process: { env: {} } },
    },
    {
      name: "React Native with a process shim",
      globals: {
        navigator: { product: "ReactNative" },
        process: { env: {}, title: "browser" },
      },
    },
    { name: "Node without navigator", globals: { process: { title: "node" } } },
    {
      name: "Node with navigator",
      globals: { process: { title: "node" }, navigator: { userAgent: "Node.js/22" } },
    },
    {
      name: "browser",
      globals: { navigator: { userAgent: "Mozilla/5.0 Chrome/130", platform: "MacIntel" } },
    },
  ])("loads and parses terminal output in $name", async ({ globals }) => {
    const context = createContext({
      ...globals,
      module: { exports: {} },
      setTimeout,
      clearTimeout,
      queueMicrotask,
      TextDecoder,
      performance,
      console,
    });

    runInContext(bundle, context, { filename: "headless-terminal-startup.cjs" });
    runInContext(
      "var terminal = module.exports.createNativeHeadlessTerminal({ rows: 2, cols: 20 });",
      context,
    );
    try {
      await runInContext('terminal.write("\\x1b[32mAit ready\\x1b[0m")', context);
      expect(
        runInContext(
          'terminal.getBufferWindow({ startRow: 0, rowCount: 1 }).rows[0].map(cell => cell.char).join("").trimEnd()',
          context,
        ),
      ).toBe("Ait ready");
    } finally {
      runInContext("terminal.dispose()", context);
    }
  });
});
