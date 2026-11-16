const { AndroidConfig, withAndroidManifest } = require("expo/config-plugins");

module.exports = (config) =>
  withAndroidManifest(config, (mod) => {
    const activity = AndroidConfig.Manifest.getMainActivityOrThrow(mod.modResults);
    const changes = new Set((activity.$["android:configChanges"] ?? "").split("|").filter(Boolean));
    // Folding changes smallestScreenWidthDp as well as screenSize/screenLayout.
    // Keep the existing React activity (and its navigation/modal state) alive.
    changes.add("smallestScreenSize");
    activity.$["android:configChanges"] = [...changes].join("|");
    return mod;
  });
