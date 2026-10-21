const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const { readFileSync } = require("node:fs");
const { dirname, join } = require("node:path");
const test = require("node:test");
const { configurePodfile } = require("./with-ios-pod-deployment-target");

const read = (file) => readFileSync(join(__dirname, "../ios", file), "utf8");

test("Pods post-install raises resource targets to the app minimum without lowering newer targets", () => {
  const podfile = read("Podfile");
  // Slice this plugin's generated block only: the surrounding post_install body now calls
  // AitBuildDiagnostics, which this Ruby harness does not stub. Whole-hook composition and
  // idempotency are covered by with-ios-build-diagnostics.test.cjs.
  const hook = podfile.match(
    /# @generated begin ait-ios-pod-deployment-target[^\n]*\n([\s\S]*?)# @generated end ait-ios-pod-deployment-target/,
  )?.[1];
  assert.ok(hook);
  const properties = read("Podfile.properties.json");
  const minimum = JSON.parse(properties)["ios.deploymentTarget"];
  const result = spawnSync(
    "ruby",
    [
      "-e",
      `
require 'json'
Configuration = Struct.new(:build_settings)
Target = Struct.new(:build_configurations)
Project = Struct.new(:targets)
Installer = Struct.new(:pods_project)
versions = ['9.0', '12.4', '13.4', nil, '${minimum}', '99.0']
targets = versions.map do |version|
  Target.new(2.times.map { Configuration.new({'IPHONEOS_DEPLOYMENT_TARGET' => version}) })
end
installer = Installer.new(Project.new(targets))
def min_ios_version_supported; '15.1'; end
podfile_properties = JSON.parse(${JSON.stringify(properties)})
${hook}
puts JSON.generate(targets.map { |target| target.build_configurations.map { |build| build.build_settings['IPHONEOS_DEPLOYMENT_TARGET'] } })
`,
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(
    JSON.parse(result.stdout),
    [minimum, minimum, minimum, minimum, minimum, "99.0"].map((version) => [version, version]),
  );
});

test("Expo prebuild preserves the Pod deployment target hook without duplicating it", () => {
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
  const configured = configurePodfile(template.stdout);
  assert.equal(configurePodfile(configured), configured);
  assert.equal(configurePodfile(read("Podfile")), read("Podfile"));
  const hook = (contents) =>
    contents.match(
      /# @generated begin ait-ios-pod-deployment-target[^\n]*\n([\s\S]*?)# @generated end ait-ios-pod-deployment-target/,
    )?.[1];
  assert.equal(hook(configured), hook(read("Podfile")));
});
