const { withPodfile } = require("expo/config-plugins");
const { mergeContents } = require("@expo/config-plugins/build/utils/generateCode");

// Two generated blocks with distinct tags: the target-level block sets the dependency warning
// policy before any pod is declared, the post_install block rewrites generated build settings.
function configurePodfile(contents) {
  const configured = mergeContents({
    src: contents,
    newSrc: `  require_relative '../plugins/ios/build_diagnostics'
  AitBuildDiagnostics.configure(current_target_definition)`,
    tag: "ait-ios-dependency-diagnostics",
    anchor: /^target ['"][^'"]+['"] do$/m,
    offset: 1,
    comment: "#",
  }).contents;
  return mergeContents({
    src: configured,
    newSrc: "    AitBuildDiagnostics.post_install(installer)",
    tag: "ait-ios-build-diagnostics",
    anchor: /^  post_install do \|installer\|$/m,
    offset: 1,
    comment: "#",
  }).contents;
}

module.exports = (config) =>
  withPodfile(config, (mod) => {
    mod.modResults.contents = configurePodfile(mod.modResults.contents);
    return mod;
  });
module.exports.configurePodfile = configurePodfile;
