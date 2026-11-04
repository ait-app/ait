import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { getNativeReleaseVersion } from "../apps/mobile/native-release-version.js";

const ios = new URL("../apps/mobile/ios/", import.meta.url);
const read = (file) => readFileSync(new URL(file, ios), "utf8");

test("committed iOS versions match the mobile release version", () => {
  const { version } = JSON.parse(
    readFileSync(new URL("../apps/mobile/package.json", import.meta.url), "utf8"),
  );
  const expected = getNativeReleaseVersion(version);
  const plist = read("Ait/Info.plist");
  for (const [key, value] of [
    ["CFBundleShortVersionString", expected.appVersion],
    ["CFBundleVersion", expected.iosBuildNumber],
  ]) {
    const actual = plist.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]+)</string>`))?.[1];
    assert.equal(actual, value, `Run Expo prebuild with the release identity and commit ${key}`);
  }
});

test("Podfile.lock pins the local ExpoTwoWayAudio package version", () => {
  const { version } = JSON.parse(
    readFileSync(new URL("../packages/expo-two-way-audio/package.json", import.meta.url), "utf8"),
  );
  const locked = read("Podfile.lock").match(/^  - ExpoTwoWayAudio \(([^)]+)\):$/m)?.[1];
  assert.equal(locked, version, "Run the pinned CocoaPods install and commit Podfile.lock");
});

test("Podfile.lock matches the committed Podfile checksum", () => {
  const checksum = createHash("sha1").update(read("Podfile")).digest("hex");
  const locked = read("Podfile.lock").match(/^PODFILE CHECKSUM: (\w+)$/m)?.[1];
  assert.equal(locked, checksum, "Run the pinned CocoaPods install and commit Podfile.lock");
});

test("Bundler pins the same CocoaPods version as Podfile.lock", () => {
  const version = read("Podfile.lock").match(/^COCOAPODS: ([\d.]+)$/m)?.[1];
  assert.ok(version);
  const gemVersion = read("Gemfile").match(/gem "cocoapods", "= ([\d.]+)"/)?.[1];
  const bundleVersion = read("Gemfile.lock").match(/^    cocoapods \(([\d.]+)\)$/m)?.[1];
  assert.equal(gemVersion, version);
  assert.equal(bundleVersion, version);
});

test("Cloud bootstrap selects locked CocoaPods over a different host version", () => {
  const root = mkdtempSync(join(tmpdir(), "ait-cloud-pods-"));
  try {
    const bin = join(root, "tools", "bin");
    mkdirSync(bin, { recursive: true });
    const files = [
      "package-lock.json",
      "apps/mobile/ios/Gemfile",
      "apps/mobile/ios/Gemfile.lock",
      "apps/mobile/ios/Podfile.lock",
      "apps/mobile/ios/Ait.xcodeproj/project.pbxproj",
      "apps/mobile/ios/Ait.xcworkspace/contents.xcworkspacedata",
      "apps/mobile/ios/Ait.xcodeproj/xcshareddata/xcschemes/Ait.xcscheme",
    ];
    for (const file of files) {
      mkdirSync(dirname(join(root, file)), { recursive: true });
      writeFileSync(join(root, file), "{}");
    }
    writeFileSync(join(root, "apps/mobile/ios/Gemfile.lock"), read("Gemfile.lock"));

    const tools = {
      brew: 'case "$1" in --prefix) printf "%s\\n" "$FIXTURE_PREFIX" ;; esac',
      node: 'printf "v22.0.0\\n"',
      ruby: 'printf "ruby 3.4\\n"',
      gem: "exit 0",
      // Stop after tool setup, before changing dependencies or invoking Xcode.
      npm: 'if [ "$1" = --version ]; then echo 10; else exit 42; fi',
      bundle: 'case "$*" in *"exec pod --version") echo LOCKED_COCOAPODS=1.16.2 ;; esac',
      pod: "echo HOST_COCOAPODS=1.17.0; exit 77",
    };
    for (const [name, body] of Object.entries(tools)) {
      writeFileSync(join(bin, name), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
    }
    const result = spawnSync("sh", [new URL("ci_scripts/ci_post_clone.sh", ios).pathname], {
      env: {
        ...process.env,
        CI_PRIMARY_REPOSITORY_PATH: root,
        FIXTURE_PREFIX: join(root, "tools"),
        PATH: `${bin}:${process.env.PATH}`,
      },
      encoding: "utf8",
    });
    assert.equal(result.status, 42, result.stdout + result.stderr);
    assert.match(result.stdout, /LOCKED_COCOAPODS=1\.16\.2/);
    assert.doesNotMatch(result.stdout, /HOST_COCOAPODS/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
