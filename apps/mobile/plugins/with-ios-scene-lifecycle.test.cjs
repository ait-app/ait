const assert = require("node:assert/strict");
const { execFileSync, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
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

test("scene connection forwards notification clicks alongside links before the JS emitter attaches", (t) => {
  if (spawnSync("swiftc", ["--version"]).status !== 0) {
    t.skip("Swift compiler is unavailable");
    return;
  }
  const generated = configureSceneAppDelegate(template);
  const start = generated.indexOf("  private func forward(_ options: UIScene.ConnectionOptions)");
  const end = generated.indexOf("  func scene(_ scene: UIScene, openURLContexts", start);
  assert.ok(start >= 0 && end > start, "Scene connection forwarding must be available");
  // Execute the generated forwarding method with platform doubles. No iOS SDK is needed:
  // the regression is losing the response before Expo can queue it for its JS emitter.
  const fixture = fs.readFileSync(
    path.join(__dirname, "ios/tests/scene-connection-forwarding.swift"),
    "utf8",
  );
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "ait-scene-notification-"));
  try {
    const source = path.join(directory, "main.swift");
    const executable = path.join(directory, "scene-test");
    fs.writeFileSync(source, fixture.replace("// FORWARD_METHOD", generated.slice(start, end)));
    execFileSync("swiftc", [
      "-module-cache-path",
      path.join(directory, "cache"),
      source,
      "-o",
      executable,
    ]);
    execFileSync(executable);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
