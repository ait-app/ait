import { buildSync } from "esbuild";
import { fileURLToPath } from "node:url";
import { createContext, runInContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const bundle = buildSync({
  stdin: {
    contents: 'import "./bootstrap"; export { appErrorReporter } from "./error-reporter";',
    resolveDir: fileURLToPath(new URL(".", import.meta.url)),
    loader: "ts",
  },
  alias: { "@": fileURLToPath(new URL("..", import.meta.url)) },
  bundle: true,
  write: false,
  platform: "neutral",
  format: "cjs",
}).outputFiles[0].text;

describe("diagnostics bootstrap in a native runtime", () => {
  it("loads the real bootstrap with React Native globals before routing", () => {
    const originalHandler = vi.fn();
    let handler = originalHandler;
    const context = createContext({
      window: {},
      navigator: { product: "ReactNative" },
      module: { exports: {} },
      console: { error: vi.fn(), warn: vi.fn() },
      queueMicrotask,
      ErrorUtils: {
        getGlobalHandler: () => handler,
        setGlobalHandler: (next: typeof handler) => {
          handler = next;
        },
      },
    });
    expect(() => runInContext(bundle, context)).not.toThrow();
    runInContext('console.error(new Error("Native startup diagnostic"))', context);
    const error = runInContext('new Error("Native runtime error")', context);
    handler(error, true);
    expect(originalHandler).toHaveBeenCalledWith(error, true);
    expect(runInContext("module.exports.appErrorReporter.takePending().length", context)).toBe(0);
    expect(
      runInContext(
        "module.exports.appErrorReporter.getReports().map(entry => entry.source)",
        context,
      ),
    ).toEqual(["console.error", "native exception"]);
  });
});
