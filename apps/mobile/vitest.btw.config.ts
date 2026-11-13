import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import path from "node:path";
import { transformSync } from "@babel/core";
import base from "./vitest.config";

const appRoot = __dirname;
const appSource = path.resolve(appRoot, "src");
const unistylesRoot = path.resolve(appRoot, "../../node_modules/react-native-unistyles");

// Keep this regression independent of unrelated native route dependencies.
export default defineConfig({
  ...base,
  plugins: [
    ...(base.plugins ?? []),
    {
      name: "btw-production-unistyles",
      enforce: "pre",
      transform(code, id) {
        const file = id.split("?")[0];
        if (!file.startsWith(appSource) || !file.endsWith(".tsx")) return null;
        // Unistyles skips its Babel visitor under NODE_ENV=test. Enable only this
        // synchronous production rewrite, then restore the test runner's environment.
        const previousEnvironment = process.env.NODE_ENV;
        Reflect.set(process.env, "NODE_ENV", "development");
        try {
          return (
            transformSync(code, {
              filename: file,
              cwd: appRoot,
              root: appRoot,
              configFile: false,
              babelrc: false,
              presets: [
                "@babel/preset-typescript",
                ["@babel/preset-react", { runtime: "automatic" }],
              ],
              plugins: [["react-native-unistyles/plugin", { root: "src" }]],
            })?.code ?? null
          );
        } finally {
          if (previousEnvironment === undefined) Reflect.deleteProperty(process.env, "NODE_ENV");
          else Reflect.set(process.env, "NODE_ENV", previousEnvironment);
        }
      },
    },
  ],
  resolve: {
    ...base.resolve,
    alias: [
      {
        find: /^lucide-react-native$/,
        replacement: path.resolve(__dirname, "test-stubs/btw-icons.web.ts"),
      },
      {
        find: /^react-native-svg$/,
        replacement: path.resolve(__dirname, "test-stubs/btw-svg.web.tsx"),
      },
      {
        find: /^react-native-unistyles\/components\/native\/(.+)$/,
        replacement: path.join(unistylesRoot, "src/components/native/$1.tsx"),
      },
      ...(base.resolve!.alias as { find: string | RegExp; replacement: string }[]).map((entry) =>
        String(entry.find) === "/^react-native-unistyles$/"
          ? { ...entry, replacement: path.join(unistylesRoot, "src/index.ts") }
          : entry,
      ),
    ],
  },
  optimizeDeps: {
    ...base.optimizeDeps,
    entries: ["src/components/provider-side-questions.browser.test.tsx"],
    include: [
      ...base.optimizeDeps!.include!,
      "@react-native/normalize-colors",
      "lucide-react-native/icons/arrow-up",
      "lucide-react-native/icons/git-branch",
      "lucide-react-native/icons/rotate-ccw-clock",
      "lucide-react-native/icons/maximize-2",
      "lucide-react-native/icons/plus",
      "lucide-react-native/icons/square",
      "lucide-react-native/icons/x",
    ],
    exclude: [
      ...base.optimizeDeps!.exclude!,
      "react-native-unistyles",
      "react-native-enriched-markdown",
    ],
  },
  test: {
    name: "btw",
    testTimeout: 30_000,
    include: ["src/components/provider-side-questions.browser.test.tsx"],
    setupFiles: [path.resolve(appRoot, "test-stubs/btw-theme-setup.ts")],
    browser: {
      enabled: true,
      provider: playwright({
        launchOptions: { args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] },
        contextOptions: { viewport: { width: 1600, height: 1000 } },
      }),
      headless: true,
      ui: false,
      screenshotDirectory: ".vitest-screenshots",
      instances: [{ browser: "chromium" }],
    },
  },
});
