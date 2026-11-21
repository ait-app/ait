const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { mkdtemp, rm } = require("node:fs/promises");
const { tmpdir } = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { evalConfig } = require("@expo/config/build/evalConfig");
const { AndroidConfig, compileModsAsync } = require("expo/config-plugins");
const withOptionalPortrait = require("./with-android-optional-portrait");

const templateArchive = path.join(
  path.dirname(require.resolve("expo/package.json")),
  "template.tgz",
);

test("app prebuild keeps the portrait lock from filtering devices on Google Play", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "ait-portrait-prebuild-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  execFileSync("tar", ["-xzf", templateArchive, "-C", root]);
  const projectRoot = path.join(root, "package");
  const manifestPath = path.join(projectRoot, "android/app/src/main/AndroidManifest.xml");
  const { config: app } = evalConfig(path.resolve(__dirname, "../app.config.js"), null);
  assert.equal(app.orientation, "portrait", "The plugin exists because the app is portrait-locked");
  assert(app.plugins.includes(withOptionalPortrait), "App config must apply the portrait plugin");

  // Repeat prebuild to cover existing native folders without duplicating the entry.
  for (let pass = 0; pass < 2; pass += 1) {
    await compileModsAsync(withOptionalPortrait({ name: app.name, slug: app.slug }), {
      projectRoot,
      platforms: ["android"],
    });
    const { manifest } = await AndroidConfig.Manifest.readAndroidManifestAsync(manifestPath);
    const portrait = (manifest["uses-feature"] ?? []).filter(
      (feature) => feature.$["android:name"] === "android.hardware.screen.portrait",
    );
    assert.deepEqual(portrait, [
      { $: { "android:name": "android.hardware.screen.portrait", "android:required": "false" } },
    ]);
  }
});
