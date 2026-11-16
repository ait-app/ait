const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { mkdtemp, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { evalConfig } = require("@expo/config/build/evalConfig");
const { AndroidConfig, compileModsAsync } = require("expo/config-plugins");
const withFoldableConfig = require("./with-android-foldable-config");

const templateArchive = path.join(
  path.dirname(require.resolve("expo/package.json")),
  "template.tgz",
);

test("app prebuild keeps the React activity alive across fold size changes", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-fold-prebuild-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync("tar", ["-xzf", templateArchive, "-C", root]);
  const projectRoot = path.join(root, "package");
  const manifestPath = path.join(projectRoot, "android/app/src/main/AndroidManifest.xml");
  const before = await AndroidConfig.Manifest.readAndroidManifestAsync(manifestPath);
  const beforeActivity = AndroidConfig.Manifest.getMainActivityOrThrow(before);
  const originalChanges = beforeActivity.$["android:configChanges"].split("|");
  const { config: app } = evalConfig(path.resolve(__dirname, "../app.config.js"), null);
  assert(app.plugins.includes(withFoldableConfig), "App config must apply the foldable plugin");

  // Repeat prebuild to cover existing native folders and preserve every Expo flag.
  for (let pass = 0; pass < 2; pass += 1) {
    await compileModsAsync(withFoldableConfig({ name: app.name, slug: app.slug }), {
      projectRoot,
      platforms: ["android"],
    });
    const manifest = await AndroidConfig.Manifest.readAndroidManifestAsync(manifestPath);
    const activity = AndroidConfig.Manifest.getMainActivityOrThrow(manifest);
    const changes = activity.$["android:configChanges"].split("|");
    assert.deepEqual(new Set(changes), new Set([...originalChanges, "smallestScreenSize"]));
    assert.equal(changes.length, new Set(changes).size);
    for (const change of ["screenSize", "screenLayout", "smallestScreenSize"]) {
      assert(changes.includes(change), `Fold configuration must handle ${change}`);
    }
  }
});
