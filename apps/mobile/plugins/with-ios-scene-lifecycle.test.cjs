const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const { readFileSync } = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");
const { configureSceneAppDelegate, configureSceneManifest } = require("./with-ios-scene-lifecycle");
const { configurePasteInputAppDelegate } = require("./with-paste-input");

// Exercise the installed SDK's real prebuild input, so an Expo template change fails here
// instead of silently leaving the app on the application-only lifecycle.
const template = execFileSync(
  "tar",
  [
    "-xOzf",
    path.join(path.dirname(require.resolve("expo/package.json")), "template.tgz"),
    "package/ios/HelloWorld/AppDelegate.swift",
  ],
  { encoding: "utf8" },
);

test("the Expo template starts React Native only after a scene supplies the window", () => {
  const result = configureSceneAppDelegate(template);
  const didFinish = result.slice(0, result.indexOf("  func startReactNativeForScene("));
  assert.match(didFinish, /bindReactNativeFactory\(factory\)/);
  assert.match(didFinish, /super.application\(application, didFinishLaunchingWithOptions:/);
  assert.doesNotMatch(didFinish, /UIWindow\(|factory.startReactNative/);
  assert.equal(result.match(/factory.startReactNative\(/g).length, 1);
  assert.match(result, /UIWindow\(windowScene: windowScene\)/);
  assert.doesNotMatch(result, /UIWindow\(frame:/);
});

test("the debug dev launcher initializes after the scene has prepared React Native", () => {
  const result = configureSceneAppDelegate(template);
  const start = result.indexOf("  func startReactNativeForScene(");
  assert.match(result.slice(0, start), /#if DEBUG\n    return true\n#else/);
  assert.match(
    result.slice(start),
    /factory.startReactNative\([\s\S]*?#if DEBUG\n[^\n]*\n    _ = super.application\(UIApplication.shared, didFinishLaunchingWithOptions: launchOptions\)/,
  );
});

for (const pasteFirst of [true, false]) {
  test(`paste-input remains after React Native startup (paste plugin first: ${pasteFirst})`, () => {
    const configure = (input) =>
      pasteFirst
        ? configureSceneAppDelegate(configurePasteInputAppDelegate(input))
        : configurePasteInputAppDelegate(configureSceneAppDelegate(input));
    const result = configure(template);
    assert.match(
      result,
      /factory.startReactNative\([\s\S]*?launchOptions: launchOptions\)\n    PasteInputModule.setup\(factory.rootViewFactory\)/,
    );
    assert.equal(result.match(/PasteInputModule.setup/g).length, 1);
    assert.equal(configure(result), result);
  });
}

test("the manifest selects the scene delegate and preserves unrelated plist configuration", () => {
  const external = [{ UISceneConfigurationName: "External display" }];
  const original = {
    CFBundleURLTypes: [{ CFBundleURLSchemes: ["ait"] }],
    UIApplicationSceneManifest: {
      UISceneConfigurations: { UIWindowSceneSessionRoleExternalDisplay: external },
    },
  };
  const result = configureSceneManifest(original);
  assert.deepEqual(result.CFBundleURLTypes, original.CFBundleURLTypes);
  assert.equal(result.UIApplicationSceneManifest.UIApplicationSupportsMultipleScenes, false);
  assert.deepEqual(
    result.UIApplicationSceneManifest.UISceneConfigurations.UIWindowSceneSessionRoleApplication,
    [
      {
        UISceneConfigurationName: "Default Configuration",
        UISceneDelegateClassName: "$(PRODUCT_MODULE_NAME).AitSceneDelegate",
      },
    ],
  );
  assert.deepEqual(
    result.UIApplicationSceneManifest.UISceneConfigurations.UIWindowSceneSessionRoleExternalDisplay,
    external,
  );
  assert.deepEqual(configureSceneManifest(result), result);
  assert.equal(original.UIApplicationSceneManifest.UIApplicationSupportsMultipleScenes, undefined);
});

test("an unsupported template fails prebuild instead of generating an application-only app", () => {
  assert.throws(
    () =>
      configureSceneAppDelegate(template.replace("factory.startReactNative(", "factory.start(")),
    /Unsupported Expo AppDelegate/,
  );
});

test("scene cold links omit the launch annotation key deprecated since the app minimum iOS", () => {
  const result = configureSceneAppDelegate(template);
  assert.doesNotMatch(result, /launchOptions\[\.annotation\]/);
  assert.match(result, /launchOptions\[\.url\] = context.url/);
  assert.match(result, /launchOptions\[\.sourceApplication\] = context.options.sourceApplication/);
  assert.match(result, /options\[\.annotation\] = context.options.annotation/);
});

// Xcode Cloud builds the committed project without running prebuild, so the generated file
// must carry the fix itself instead of relying on the plugin template.
test("the committed AppDelegate carries the same cold-link fix", () => {
  const committed = readFileSync(path.join(__dirname, "../ios/Ait/AppDelegate.swift"), "utf8");
  assert.doesNotMatch(committed, /launchOptions\[\.annotation\]/);
  assert.match(committed, /launchOptions\[\.url\] = context\.url/);
  assert.match(
    committed,
    /launchOptions\[\.sourceApplication\] = context\.options\.sourceApplication/,
  );
});
