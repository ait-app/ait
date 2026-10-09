Pod::Spec.new do |s|
  s.name = 'AitTerminalInput'
  s.version = '0.1.0'
  s.summary = 'IME-aware iOS terminal input for Ait'
  s.description = 'UIKit owns terminal composition and emits only committed text.'
  s.license = 'Apache-2.0'
  s.author = 'Ait'
  s.homepage = 'https://github.com/ait-app/ait'
  s.platforms = { :ios => '15.1' }
  s.swift_version = '5.4'
  s.source = { :path => '.' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
  s.source_files = '*.swift'
end
