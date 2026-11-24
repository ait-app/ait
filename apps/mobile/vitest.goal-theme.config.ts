import { defineConfig } from "vitest/config";
import base, { productionUnistylesPlugin } from "./vitest.btw.config";
import common from "./vitest.config";

// Reuse the production theme runtime and compiler without changing the BTW test scope.
export default defineConfig({
  ...base,
  plugins: [...(common.plugins ?? []), productionUnistylesPlugin(true)],
  test: {
    ...base.test,
    name: "goal-theme",
    include: ["src/composer/goal-theme.browser.tsx"],
  },
});
