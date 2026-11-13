import { defineConfig } from "vitest/config";
import base from "./vitest.btw.config";

// Scope dependency discovery to the shared renderer instead of unrelated native routes.
export default defineConfig({
  ...base,
  optimizeDeps: {
    ...base.optimizeDeps,
    entries: ["src/components/markdown/renderer.browser.test.tsx"],
  },
  test: {
    ...base.test,
    name: "markdown",
    include: ["src/components/markdown/renderer.browser.test.tsx"],
  },
});
