const fs = require("node:fs");
const path = require("node:path");
const { withAppDelegate, withInfoPlist } = require("expo/config-plugins");

const SCENE_START = "// @generated Ait scene lifecycle begin";
const SCENE_END = "// @generated Ait scene lifecycle end";
const FACTORY_PROPERTY = "  var reactNativeFactory: RCTReactNativeFactory?";
const FACTORY_BIND = "    bindReactNativeFactory(factory)";
const LINKING_SECTION = "  // Linking API";
const WINDOW_START = "    window = UIWindow(frame: UIScreen.main.bounds)\n";
const FINISH_LAUNCH =
  "    return super.application(application, didFinishLaunchingWithOptions: launchOptions)";

function configureSceneManifest(infoPlist) {
  const manifest = infoPlist.UIApplicationSceneManifest ?? {};
  return {
    ...infoPlist,
    UIApplicationSceneManifest: {
      ...manifest,
      UIApplicationSupportsMultipleScenes: false,
      UISceneConfigurations: {
        ...manifest.UISceneConfigurations,
        UIWindowSceneSessionRoleApplication: [
          {
            UISceneConfigurationName: "Default Configuration",
            UISceneDelegateClassName: "$(PRODUCT_MODULE_NAME).AitSceneDelegate",
          },
        ],
      },
    },
  };
}

function configureSceneAppDelegate(contents) {
  let configured = contents;
  if (!configured.includes("  func startReactNativeForScene(")) {
    const startup = configured.match(/#if os\(iOS\) \|\| os\(tvOS\)\n[\s\S]*?#endif/);
    if (
      !startup?.[0].includes(WINDOW_START) ||
      !startup[0].includes("factory.startReactNative(") ||
      !configured.includes(FACTORY_PROPERTY) ||
      !configured.includes(FACTORY_BIND) ||
      !configured.includes(FINISH_LAUNCH) ||
      !configured.includes(LINKING_SECTION)
    ) {
      throw new Error("Unsupported Expo AppDelegate: could not migrate React Native to UIScene");
    }
    // Retain the startup body (including other plugins' post-start hooks), but the scene owns
    // window creation. Keeping the Expo launchOptions anchor also lets paste-input run later.
    const startMethod = `  func startReactNativeForScene(
    in window: UIWindow,
    launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) {
    guard let factory = reactNativeFactory else { return }
${startup[0].replace(WINDOW_START, "")}
#if DEBUG
    // Expo 54's dev launcher requires a prepared React root and a visible window.
    _ = super.application(UIApplication.shared, didFinishLaunchingWithOptions: launchOptions)
#endif
  }

`;
    configured = configured
      .replace(startup[0] + "\n\n", "")
      .replace(
        FACTORY_PROPERTY,
        FACTORY_PROPERTY + "\n  var initialLaunchOptions: [UIApplication.LaunchOptionsKey: Any]?",
      )
      .replace(FACTORY_BIND, FACTORY_BIND + "\n    initialLaunchOptions = launchOptions")
      .replace(FINISH_LAUNCH, `#if DEBUG\n    return true\n#else\n${FINISH_LAUNCH}\n#endif`)
      .replace(LINKING_SECTION, startMethod + LINKING_SECTION);
  }
  const sceneSource = fs.readFileSync(path.join(__dirname, "ios/AitSceneDelegate.swift"), "utf8");
  const sceneBlock = `${SCENE_START}\n${sceneSource.trimEnd()}\n${SCENE_END}`;
  const previous = new RegExp(`${SCENE_START}[\\s\\S]*?${SCENE_END}`);
  return previous.test(configured)
    ? configured.replace(previous, () => sceneBlock)
    : `${configured.trimEnd()}\n\n${sceneBlock}\n`;
}

function withIosSceneLifecycle(config) {
  config = withInfoPlist(config, (modConfig) => {
    modConfig.modResults = configureSceneManifest(modConfig.modResults);
    return modConfig;
  });
  return withAppDelegate(config, (modConfig) => {
    if (modConfig.modResults.language !== "swift") {
      throw new Error("Ait scene lifecycle requires a Swift AppDelegate");
    }
    modConfig.modResults.contents = configureSceneAppDelegate(modConfig.modResults.contents);
    return modConfig;
  });
}

module.exports = withIosSceneLifecycle;
module.exports.configureSceneManifest = configureSceneManifest;
module.exports.configureSceneAppDelegate = configureSceneAppDelegate;
