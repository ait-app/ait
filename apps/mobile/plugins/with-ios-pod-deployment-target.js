const { withPodfile } = require("expo/config-plugins");
const { mergeContents } = require("@expo/config-plugins/build/utils/generateCode");

function configurePodfile(contents) {
  return mergeContents({
    src: contents,
    newSrc: `    # Resource bundles can retain older Podspec minimums after React Native's post-install.
    minimum_target = Gem::Version.new(podfile_properties['ios.deploymentTarget'] || min_ios_version_supported)
    installer.pods_project.targets.each do |pod_target|
      pod_target.build_configurations.each do |build_config|
        current_target = build_config.build_settings['IPHONEOS_DEPLOYMENT_TARGET']
        if current_target.nil? || Gem::Version.new(current_target) < minimum_target
          build_config.build_settings['IPHONEOS_DEPLOYMENT_TARGET'] = minimum_target.to_s
        end
      end
    end`,
    tag: "ait-ios-pod-deployment-target",
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
