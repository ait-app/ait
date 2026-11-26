import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import path from "node:path";
import base from "./vitest.config";

// Render real Lucide paths when verifying feature state colors; generic tests omit icons.
export default defineConfig({
  ...base,
  resolve: {
    ...base.resolve,
    alias: [
      {
        find: /^lucide-react-native$/,
        replacement: path.resolve(__dirname, "test-stubs/vibe-icons.web.ts"),
      },
      {
        find: /^react-native-svg$/,
        replacement: path.resolve(__dirname, "test-stubs/btw-svg.web.tsx"),
      },
      ...(base.resolve!.alias as { find: string | RegExp; replacement: string }[]),
    ],
  },
  optimizeDeps: {
    ...base.optimizeDeps,
    include: [
      ...base.optimizeDeps!.include!,
      ...["bot", "zap", "settings-2", "check"].map((name) => `lucide-react-native/icons/${name}`),
    ],
  },
  test: {
    name: "vibe-browser",
    testTimeout: 60_000,
    hookTimeout: 60_000,
    include: [
      "src/composer/vibe.browser.test.tsx",
      "src/composer/agent-controls/features-menu.browser.test.tsx",
    ],
    browser: {
      enabled: true,
      provider: playwright(),
      headless: true,
      instances: [{ browser: "chromium" }],
      screenshotDirectory: ".vitest-screenshots",
    },
  },
});
