import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import base from "./vitest.config";

const consumer = "src/components/usage/session-statistics.browser.test.tsx";

/** Exercise the statistics consumer without pre-bundling unrelated native route fixtures. */
export default defineConfig({
  ...base,
  optimizeDeps: { ...base.optimizeDeps, entries: [consumer] },
  test: {
    name: "statistics",
    include: [consumer],
    browser: {
      enabled: true,
      provider: playwright(),
      headless: true,
      instances: [{ browser: "chromium" }],
    },
  },
});
