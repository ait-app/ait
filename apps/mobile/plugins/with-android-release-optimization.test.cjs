const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { mkdtemp, readFile, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { evalConfig } = require("@expo/config/build/evalConfig");
const { compileModsAsync } = require("expo/config-plugins");
const { withBuildProperties } = require("expo-build-properties");
const withReleaseOptimization = require("./with-android-release-optimization");
const { configureReleaseOptimization } = withReleaseOptimization;

const templateArchive = path.join(
  path.dirname(require.resolve("expo/package.json")),
  "template.tgz",
);
const template = execFileSync(
  "tar",
  ["-xOzf", templateArchive, "package/android/app/build.gradle"],
  {
    encoding: "utf8",
  },
);

test("the installed Expo template keeps app rules and debug behavior while enabling optimization", () => {
  const result = configureReleaseOptimization(template);
  assert.equal(
    result,
    template.replace('"proguard-android.txt"', '"proguard-android-optimize.txt"'),
  );
  assert.equal(configureReleaseOptimization(result), result);
  assert.match(result, /proguardFiles .*"proguard-rules.pro"/);
});

test("unknown or ambiguous default rules fail prebuild rather than disabling optimization", () => {
  for (const source of [
    template.replace("proguard-android.txt", "custom-defaults.txt"),
    template + '\ngetDefaultProguardFile("proguard-android.txt")',
  ]) {
    assert.throws(() => configureReleaseOptimization(source), /Unsupported Expo Gradle template/);
  }
});

test("the app's Expo options generate optimized release properties accepted by Play CI", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-r8-prebuild-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync("tar", ["-xzf", templateArchive, "-C", root]);
  const projectRoot = path.join(root, "package");
  const { config: exp } = evalConfig(path.resolve(__dirname, "../app.config.js"), null);
  assert(exp.plugins.includes(withReleaseOptimization), "App config must apply the R8 plugin");
  const [, options] = exp.plugins.find((plugin) => plugin[0] === "expo-build-properties");
  const config = withReleaseOptimization(
    withBuildProperties({ name: exp.name, slug: exp.slug }, options),
  );
  await compileModsAsync(config, { projectRoot, platforms: ["android"] });
  const gradle = await readFile(path.join(projectRoot, "android/app/build.gradle"), "utf8");
  const properties = await readFile(path.join(projectRoot, "android/gradle.properties"), "utf8");
  const { validateAndroidPlayOptimization } =
    await import("../../../scripts/android-play-release.mjs");
  validateAndroidPlayOptimization(gradle, properties);
  assert.match(gradle, /minifyEnabled enableMinifyInReleaseBuilds/);
  assert.match(gradle, /shrinkResources enableShrinkResources.toBoolean\(\)/);
});
