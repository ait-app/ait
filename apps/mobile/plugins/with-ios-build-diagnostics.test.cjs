const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const { readFileSync, readdirSync } = require("node:fs");
const { dirname, join } = require("node:path");
const test = require("node:test");
const { configurePodfile } = require("./with-ios-build-diagnostics");
const { configurePodfile: configureMinimum } = require("./with-ios-pod-deployment-target");

const readPodfile = () => readFileSync(join(__dirname, "../ios/Podfile"), "utf8");
const readProject = () =>
  readFileSync(join(__dirname, "../ios/Ait.xcodeproj/project.pbxproj"), "utf8");

test("diagnostics survive Expo prebuild and compose with the deployment minimum plugin", () => {
  const template = spawnSync(
    "tar",
    [
      "-xOzf",
      join(dirname(require.resolve("expo/package.json")), "template.tgz"),
      "package/ios/Podfile",
    ],
    { encoding: "utf8" },
  );
  assert.equal(template.status, 0, template.stderr);
  for (const input of [template.stdout, configureMinimum(template.stdout), readPodfile()]) {
    const configured = configurePodfile(input);
    assert.equal(configurePodfile(configured), configured);
    assert.equal(configureMinimum(configureMinimum(configured)), configureMinimum(configured));
    assert.match(configured, /require_relative '\.\.\/plugins\/ios\/build_diagnostics'/);
    assert.equal(
      configured.match(/AitBuildDiagnostics\.configure\(current_target_definition\)/g)?.length ?? 0,
      1,
    );
    assert.equal(
      configured.match(/AitBuildDiagnostics\.post_install\(installer\)/g)?.length ?? 0,
      1,
    );
  }
  assert.equal(configurePodfile(readPodfile()), readPodfile());
});

test("the committed app target matches the Swift importer diagnostics list", () => {
  const result = spawnSync(
    "ruby",
    [
      "-e",
      "require ARGV.fetch(0); puts AitBuildDiagnostics::IMPORT_DIAGNOSTICS.map { |flag| ['-Xcc', flag] }.flatten.join(' ')",
      join(__dirname, "ios/build_diagnostics.rb"),
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  const expected = result.stdout.trim();
  assert.match(expected, /^-Xcc /);
  const swiftFlags = [...readProject().matchAll(/OTHER_SWIFT_FLAGS = "([^"]*)";/g)].map(
    (match) => match[1],
  );
  assert.equal(swiftFlags.length, 2);
  for (const value of swiftFlags) {
    assert.ok(value.endsWith(expected), value);
  }
});

test("Ruby policy preserves owned diagnostics, custom flags, paths and unrelated build phases", () => {
  const result = spawnSync(
    "ruby",
    [
      "-e",
      `
require 'json'
require ARGV.fetch(0)
class Definition
  attr_accessor :inhibit_all_warnings
  def initialize; @exceptions = {}; end
  def set_inhibit_warnings_for_pod(name, flag); @exceptions[name] = flag; end
  def inhibits?(name); @exceptions.fetch(name, inhibit_all_warnings); end
end
Configuration = Struct.new(:build_settings)
UserTarget = Struct.new(:build_configurations)
Project = Struct.new(:save_count) do
  def save; self.save_count += 1; end
end
class Xcconfig
  attr_reader :attributes, :saved_paths
  def initialize(paths); @attributes = {'LIBRARY_SEARCH_PATHS' => paths}; @saved_paths = []; end
  def save_as(path); @saved_paths << path; end
end
Aggregate = Struct.new(:xcconfigs, :user_targets, :user_project) do
  def xcconfig_path(name); "generated/#{name}.xcconfig"; end
end
Phase = Struct.new(:name, :always_out_of_date)
PodTarget = Struct.new(:name, :shell_script_build_phases, :product_type, :build_configurations)
PodProject = Struct.new(:targets)
Installer = Struct.new(:aggregate_targets, :pods_project)
input = JSON.parse(STDIN.read)
definition = Definition.new
AitBuildDiagnostics.configure(definition)
configs = input['flags'].map { |flags| Configuration.new({'OTHER_SWIFT_FLAGS' => flags}) }
xcconfig = Xcconfig.new(input['paths'])
aggregate = Aggregate.new({'Debug' => xcconfig}, [UserTarget.new(configs)], Project.new(0))
hermes_phase = Phase.new('[CP-User] [Hermes] Replace Hermes for the right configuration, if needed', nil)
other_phase = Phase.new('Unrelated build phase', nil)
vendor = PodTarget.new('VendorSDK', [], 'com.apple.product-type.library.static', [Configuration.new({})])
owned = PodTarget.new('ExpoTwoWayAudio', [], 'com.apple.product-type.library.static', [Configuration.new({})])
owned_subspec = PodTarget.new('PaseoHardwareKeyboard-Legacy', [], 'com.apple.product-type.library.static', [Configuration.new({})])
resource = PodTarget.new('Vendor-Resources', [], 'com.apple.product-type.bundle', [Configuration.new({})])
installer = Installer.new([aggregate], PodProject.new([PodTarget.new('hermes-engine', [hermes_phase, other_phase]), vendor, owned, owned_subspec, resource]))
2.times { AitBuildDiagnostics.post_install(installer) }
puts JSON.generate({
  owned: AitBuildDiagnostics::OWNED_PODS,
  policy: ['ExpoTwoWayAudio', 'PaseoHardwareKeyboard', 'React-Core', 'FutureDependency'].to_h { |name| [name, definition.inhibits?(name)] },
  configs: configs.map(&:build_settings),
  paths: xcconfig.attributes['LIBRARY_SEARCH_PATHS'],
  saved: xcconfig.saved_paths,
  hermes: hermes_phase.always_out_of_date,
  other: other_phase.always_out_of_date,
  archives: [vendor, owned, owned_subspec, resource].map { |target| target.build_configurations.first.build_settings }
})
`,
      join(__dirname, "ios/build_diagnostics.rb"),
    ],
    {
      input: JSON.stringify({
        flags: [
          '$(inherited) -D "KEEP ME"',
          ["$(inherited)", "-Xcc", "-Wno-incomplete-umbrella"],
          "$(inherited) -Wno-incomplete-umbrella",
        ],
        paths:
          '$(inherited) "${TOOLCHAIN_DIR}/usr/lib/swift/${PLATFORM_NAME}" "custom library path" /usr/lib/swift',
      }),
      encoding: "utf8",
    },
  );
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const observed = JSON.parse(result.stdout);
  assert.deepEqual(observed.policy, {
    ExpoTwoWayAudio: false,
    PaseoHardwareKeyboard: false,
    "React-Core": true,
    FutureDependency: true,
  });
  assert.deepEqual(observed.configs[0], {
    OTHER_SWIFT_FLAGS:
      '$(inherited) -D "KEEP ME" -Xcc -Wno-incomplete-umbrella -Xcc -Wno-nullability-completeness',
  });
  assert.deepEqual(observed.configs[1], {
    OTHER_SWIFT_FLAGS: [
      "$(inherited)",
      "-Xcc",
      "-Wno-incomplete-umbrella",
      "-Xcc",
      "-Wno-nullability-completeness",
    ],
  });
  // A flag that appears without its `-Xcc` prefix must not suppress the whole pair.
  assert.deepEqual(observed.configs[2], {
    OTHER_SWIFT_FLAGS:
      "$(inherited) -Wno-incomplete-umbrella -Xcc -Wno-incomplete-umbrella -Xcc -Wno-nullability-completeness",
  });
  assert.equal(
    observed.paths,
    '$(inherited) "${SDKROOT}/usr/lib/swift" "custom library path" /usr/lib/swift',
  );
  // The second install finds nothing left to rewrite instead of writing the xcconfig again.
  assert.deepEqual(observed.saved, ["generated/Debug.xcconfig"]);
  assert.match(result.stderr, /No aggregate xcconfig referenced/);
  assert.equal(observed.hermes, "1");
  assert.equal(observed.other, null);
  assert.deepEqual(observed.archives, [
    { OTHER_LIBTOOLFLAGS: "$(inherited) -no_warning_for_no_symbols" },
    {},
    {},
    {},
  ]);

  // New Ait-owned native modules must opt back into diagnostics as well. Walking the whole
  // package and module trees keeps the check independent of where a podspec is anchored.
  const owned = [];
  const collectPodspecs = (root) => {
    for (const entry of readdirSync(root, { withFileTypes: true })) {
      if (entry.name === "node_modules") continue;
      const path = join(root, entry.name);
      if (entry.isDirectory()) {
        collectPodspecs(path);
        continue;
      }
      if (!entry.name.endsWith(".podspec")) continue;
      const name = readFileSync(path, "utf8").match(/\.name\s*=\s*["']([^"']+)/)?.[1];
      assert.ok(name, path);
      owned.push(name);
    }
  };
  for (const root of [join(__dirname, "../../../packages"), join(__dirname, "../modules")]) {
    collectPodspecs(root);
  }
  assert.deepEqual(observed.owned.sort(), owned.sort());
});
