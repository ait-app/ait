const { withAppBuildGradle } = require("expo/config-plugins");

function configureReleaseOptimization(contents) {
  const defaultRules = /getDefaultProguardFile\((["'])proguard-android(?:-optimize)?\.txt\1\)/g;
  if ([...contents.matchAll(defaultRules)].length !== 1) {
    throw new Error("Unsupported Expo Gradle template: expected one default ProGuard rules file");
  }
  // The legacy default includes -dontoptimize; keep the app/library rules intact.
  return contents.replace(defaultRules, 'getDefaultProguardFile("proguard-android-optimize.txt")');
}

module.exports = (config) =>
  withAppBuildGradle(config, (mod) => {
    mod.modResults.contents = configureReleaseOptimization(mod.modResults.contents);
    return mod;
  });
module.exports.configureReleaseOptimization = configureReleaseOptimization;
