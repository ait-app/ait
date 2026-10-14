require 'tmpdir'
require 'xcodeproj'

device = ARGV.fetch(0) { abort 'Usage: ruby run-ios-tests.rb <simulator-udid>' }
module_root = File.expand_path('..', __dir__)

# Build only the UIKit input and its regressions, without an Expo application,
# credentials, a daemon, or an installed app. All products stay in a temp dir.
Dir.mktmpdir('ait-terminal-input-tests-') do |directory|
  project = Xcodeproj::Project.new(File.join(directory, 'TerminalInput.xcodeproj'))
  host = project.new_target(:application, 'TerminalInputHost', :ios, '17.0')
  tests = project.new_target(:unit_test_bundle, 'TerminalInputTests', :ios, '17.0')
  tests.add_dependency(host)

  [host, tests].each do |target|
    target.build_configurations.each do |config|
      config.build_settings['SWIFT_VERSION'] = '5.0'
      config.build_settings['GENERATE_INFOPLIST_FILE'] = 'YES'
      config.build_settings['CODE_SIGNING_ALLOWED'] = 'NO'
      config.build_settings['PRODUCT_BUNDLE_IDENTIFIER'] = "dev.ait.#{target.name}"
      config.build_settings['TARGETED_DEVICE_FAMILY'] = '1,2'
    end
  end
  tests.build_configurations.each do |config|
    config.build_settings['TEST_HOST'] = '$(BUILT_PRODUCTS_DIR)/TerminalInputHost.app/TerminalInputHost'
    config.build_settings['BUNDLE_LOADER'] = '$(TEST_HOST)'
  end

  app_delegate = File.join(directory, 'AppDelegate.swift')
  File.write(app_delegate, <<~SWIFT)
    import UIKit
    @main final class AppDelegate: UIResponder, UIApplicationDelegate {
      var window: UIWindow?
      func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        window = UIWindow(frame: UIScreen.main.bounds)
        window?.rootViewController = UIViewController()
        window?.makeKeyAndVisible()
        return true
      }
    }
  SWIFT
  host.add_file_references([project.main_group.new_file(app_delegate)])
  tests.add_file_references([
    project.main_group.new_file(File.join(module_root, 'ios', 'TerminalTextView.swift')),
    project.main_group.new_file(File.join(__dir__, 'TerminalTextViewTests.swift'))
  ])
  project.save

  scheme = Xcodeproj::XCScheme.new
  scheme.add_build_target(host)
  scheme.add_build_target(tests)
  scheme.set_launch_target(host)
  scheme.add_test_target(tests)
  scheme.save_as(project.path, 'TerminalInputTests', true)

  result = File.join(directory, 'TerminalInput.xcresult')
  log = File.join(directory, 'xcodebuild.log')
  args = ['xcodebuild', '-project', project.path.to_s,
    '-scheme', 'TerminalInputTests', '-destination', "platform=iOS Simulator,id=#{device}",
    '-derivedDataPath', File.join(directory, 'DerivedData'),
    '-resultBundlePath', result,
    '-parallel-testing-enabled', 'NO', '-collect-test-diagnostics', 'never', 'test']
  args << "-only-testing:TerminalInputTests/TerminalTextViewTests/#{ARGV[1]}" if ARGV[1]
  success = system(*args, out: log, err: [:child, :out])
  puts File.readlines(log).select { |line| line.match?(/Test (Case|Suite)|error:|XCTAssert|failed|passed/) }
  system('xcrun', 'xcresulttool', 'get', 'test-results', 'summary', '--path', result) if File.exist?(result)
  abort 'iOS terminal input tests failed' unless success
end
