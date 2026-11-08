import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import path from "node:path";
import base from "./vitest.config";

// Theme integration tests need the real web runtime, not the fixed-theme unit stub.
export default defineConfig({
  ...base,
  resolve: {
    ...base.resolve,
    alias: (base.resolve!.alias as { find: string | RegExp; replacement: string }[]).map((entry) =>
      String(entry.find) === "/^react-native-unistyles$/"
        ? {
            ...entry,
            replacement: path.resolve(
              __dirname,
              "../../node_modules/react-native-unistyles/src/index.ts",
            ),
          }
        : entry,
    ),
  },
  optimizeDeps: {
    ...base.optimizeDeps,
    include: [...base.optimizeDeps!.include!, "@react-native/normalize-colors"],
    exclude: [
      ...base.optimizeDeps!.exclude!,
      "react-native-unistyles",
      "react-native-enriched-markdown",
    ],
  },
  test: {
    name: "appearance",
    include: ["src/appearance/system-theme.browser.tsx"],
    browser: {
      enabled: true,
      provider: playwright(),
      headless: true,
      screenshotDirectory: ".vitest-screenshots",
      instances: [{ browser: "chromium" }],
      commands: {
        async setSystemColorScheme({ page }, colorScheme: "light" | "dark") {
          await page.emulateMedia({ colorScheme });
        },
      },
    },
  },
});
