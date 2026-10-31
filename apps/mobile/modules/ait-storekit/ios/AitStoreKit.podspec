Pod::Spec.new do |s|
  s.name = 'AitStoreKit'
  s.version = '0.1.0'
  s.summary = 'Ait Online Service StoreKit subscriptions'
  s.description = 'StoreKit 2 purchases with server-verified account entitlements'
  s.license = 'Apache-2.0'
  s.author = 'Ait'
  s.homepage = 'https://ait-app.com'
  s.platforms = { :ios => '16.0' }
  s.swift_version = '5.4'
  s.source = { :path => '.' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.frameworks = 'StoreKit'
  s.source_files = '**/*.{h,m,swift}'
end
