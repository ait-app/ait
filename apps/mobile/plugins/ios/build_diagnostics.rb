require 'shellwords'

# Keep diagnostics for Ait-owned code, while treating installed SDKs as dependencies.
module AitBuildDiagnostics
  OWNED_PODS = %w[ExpoTwoWayAudio PaseoHardwareKeyboard].freeze
  IMPORT_DIAGNOSTICS = %w[-Wno-incomplete-umbrella -Wno-nullability-completeness].freeze

  def self.configure(target_definition)
    target_definition.inhibit_all_warnings = true
    OWNED_PODS.each do |name|
      target_definition.set_inhibit_warnings_for_pod(name, false)
    end
  end

  def self.post_install(installer)
    installer.aggregate_targets.each do |aggregate|
      aggregate.xcconfigs.each do |configuration, xcconfig|
        # CocoaPods 1.16 uses TOOLCHAIN_DIR, which may select Metal in Xcode Cloud.
        # Swift SDK libraries belong to SDKROOT, not the selected Metal toolchain.
        paths = xcconfig.attributes['LIBRARY_SEARCH_PATHS']
        next unless paths
        xcconfig.attributes['LIBRARY_SEARCH_PATHS'] = paths.gsub(
          '${TOOLCHAIN_DIR}/usr/lib/swift/${PLATFORM_NAME}', '${SDKROOT}/usr/lib/swift'
        )
        xcconfig.save_as(aggregate.xcconfig_path(configuration))
      end

      aggregate.user_targets.each do |target|
        target.build_configurations.each do |configuration|
          # Expo 54 / RN 0.81's imported headers generate these two diagnostics.
          # This only affects the Swift Clang importer; Swift and ObjC source warnings remain enabled.
          append_options(configuration.build_settings, 'OTHER_SWIFT_FLAGS',
                         IMPORT_DIAGNOSTICS.map { |flag| ['-Xcc', flag] })
        end
      end
      aggregate.user_project.save
    end

    installer.pods_project.targets.each do |target|
      next unless target.respond_to?(:product_type) && target.product_type == 'com.apple.product-type.library.static'
      next if OWNED_PODS.any? { |name| target.name == name || target.name.start_with?("#{name}-") }
      target.build_configurations.each do |configuration|
        # Platform-gated and generated vendor files can intentionally have no exported symbols.
        append_options(configuration.build_settings, 'OTHER_LIBTOOLFLAGS', [['-no_warning_for_no_symbols']])
      end
    end

    hermes = installer.pods_project.targets.find { |target| target.name == 'hermes-engine' }
    hermes&.shell_script_build_phases&.each do |phase|
      # The version selector must run even when an earlier build changed Hermes configuration.
      phase.always_out_of_date = '1' if phase.name&.include?('[Hermes] Replace Hermes')
    end
  end

  def self.append_options(settings, key, options)
    flags = settings[key] || '$(inherited)'
    tokens = flags.is_a?(Array) ? flags : Shellwords.split(flags)
    extra = options.reject { |option| tokens.include?(option.last) }.flatten
    settings[key] = flags.is_a?(Array) ? flags + extra : [flags, *extra].join(' ')
  end
end
