const { withAndroidManifest } = require("expo/config-plugins");

const PORTRAIT_FEATURE = "android.hardware.screen.portrait";

module.exports = (config) =>
  withAndroidManifest(config, (mod) => {
    const manifest = mod.modResults.manifest;
    // A portrait-locked activity implies this feature as required, so Google Play
    // hides the app from devices that do not report it (Chromebooks, desktop
    // windowing). The lock is a phone layout preference, not a hardware need.
    manifest["uses-feature"] = [
      ...(manifest["uses-feature"] ?? []).filter(
        (feature) => feature.$["android:name"] !== PORTRAIT_FEATURE,
      ),
      { $: { "android:name": PORTRAIT_FEATURE, "android:required": "false" } },
    ];
    return mod;
  });
