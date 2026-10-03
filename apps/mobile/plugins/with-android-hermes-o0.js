const { withAppBuildGradle } = require("expo/config-plugins");
const { mergeContents } = require("@expo/config-plugins/build/utils/generateCode");

module.exports = (config) =>
  withAppBuildGradle(config, (mod) => {
    if (mod.modResults.language !== "groovy") {
      throw new Error("Android Hermes -O0 requires a Groovy app build.gradle");
    }
    mod.modResults.contents = mergeContents({
      src: mod.modResults.contents,
      newSrc: '    hermesFlags = ["-O0", "-output-source-map"]',
      tag: "ait-android-hermes-o0",
      anchor: /^react\s*\{/m,
      offset: 1,
      comment: "//",
    }).contents;
    return mod;
  });
