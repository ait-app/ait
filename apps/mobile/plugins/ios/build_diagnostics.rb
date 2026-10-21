require 'shellwords'

# Keep diagnostics for Ait-owned code, while treating installed SDKs as dependencies.
module AitBuildDiagnostics
  OWNED_PODS = %w[ExpoTwoWayAudio PaseoHardwareKeyboard].freeze
  IMPORT_DIAGNOSTICS = %w[-Wno-incomplete-umbrella -Wno-nullability-completeness].freeze
  TOOLCHAIN_SWIFT_PATH = '${TOOLCHAIN_DIR}/usr/lib/swift/${PLATFORM_NAME}'
  SDKROOT_SWIFT_PATH = '${SDKROOT}/usr/lib/swift'

  def self.configure(target_definition)
    target_definition.inhibit_all_warnings = true
    OWNED_PODS.each do |name|
      target_definition.set_inhibit_warnings_for_pod(name, false)
    end
  end

  def self.post_install(installer)
    rewrite_swift_library_search_paths(installer.aggregate_targets)
    append_swift_importer_diagnostics(installer.aggregate_targets)
    silence_empty_static_libraries(installer.pods_project)
    always_run_hermes_scripts(installer.pods_project)
  end

  # CocoaPods 1.16 points Swift libraries at TOOLCHAIN_DIR, which may select the Metal
  # toolchain on Xcode Cloud; Swift SDK libraries belong to SDKROOT. Only the aggregate
  # xcconfigs are rewritten because the app links through them while it builds static
  # libraries. Enabling `ios.useFrameworks`/`USE_FRAMEWORKS` would additionally need the
  # pod targets and CocoaPods' embed frameworks script covered.
  def self.rewrite_swift_library_search_paths(aggregate_targets)
    rewritten = 0
    aggregate_targets.each do |aggregate|
      aggregate.xcconfigs.each do |configuration, xcconfig|
        paths = xcconfig.attributes['LIBRARY_SEARCH_PATHS']
        next unless paths
        replacement = paths.gsub(TOOLCHAIN_SWIFT_PATH, SDKROOT_SWIFT_PATH)
        next if replacement == paths
        rewritten += 1
        xcconfig.attributes['LIBRARY_SEARCH_PATHS'] = replacement
        xcconfig.save_as(aggregate.xcconfig_path(configuration))
      end
    end
    warn_unless_rewritten(rewritten)
  end

  # Fails loudly instead of becoming a silent no-op: without the rewrite, Xcode Cloud
  # resolves TOOLCHAIN_DIR to the Metal toolchain and loses the Swift SDK libraries.
  def self.warn_unless_rewritten(rewritten)
    return unless rewritten.zero?
    message = "[Ait] No aggregate xcconfig referenced #{TOOLCHAIN_SWIFT_PATH}; " \
              'the Xcode Cloud Swift library search path fix no longer applies.'
    if defined?(Pod::UI)
      Pod::UI.warn(message)
    else
      Kernel.warn(message)
    end
  end

  # Expo 54 / RN 0.81's imported headers generate these two diagnostics. This only affects
  # the Swift Clang importer; Swift and ObjC source warnings remain enabled.
  def self.append_swift_importer_diagnostics(aggregate_targets)
    aggregate_targets.each do |aggregate|
      aggregate.user_targets.each do |target|
        target.build_configurations.each do |configuration|
          append_options(configuration.build_settings, 'OTHER_SWIFT_FLAGS',
                         IMPORT_DIAGNOSTICS.map { |flag| ['-Xcc', flag] })
        end
      end
      aggregate.user_project.save
    end
  end

  # Platform-gated and generated vendor files can intentionally export no symbols.
  def self.silence_empty_static_libraries(pods_project)
    pods_project.targets.each do |target|
      next unless target.respond_to?(:product_type) && target.product_type == 'com.apple.product-type.library.static'
      next if owned_pod?(target.name)
      target.build_configurations.each do |configuration|
        append_options(configuration.build_settings, 'OTHER_LIBTOOLFLAGS', [['-no_warning_for_no_symbols']])
      end
    end
  end

  def self.owned_pod?(name)
    OWNED_PODS.any? { |owned| name == owned || name.start_with?("#{owned}-") }
  end

  # The version selector must run even when an earlier build changed Hermes configuration;
  # this also removes the script phase's missing-output warning.
  def self.always_run_hermes_scripts(pods_project)
    hermes = pods_project.targets.find { |target| target.name == 'hermes-engine' }
    hermes&.shell_script_build_phases&.each do |phase|
      phase.always_out_of_date = '1' if phase.name&.include?('[Hermes] Replace Hermes')
    end
  end

  # Appends whole flag sequences so an existing `-Xcc` pair is never half-matched, and
  # leaves the list untouched when every sequence is already present.
  def self.append_options(settings, key, sequences)
    flags = settings[key] || '$(inherited)'
    tokens = flags.is_a?(Array) ? flags : Shellwords.split(flags)
    extra = sequences.reject { |sequence| includes_sequence?(tokens, sequence) }.flatten
    return if extra.empty?
    settings[key] = flags.is_a?(Array) ? flags + extra : [flags, *extra].join(' ')
  end

  def self.includes_sequence?(tokens, sequence)
    return tokens.include?(sequence.first) if sequence.one?
    tokens.each_cons(sequence.length).any? { |window| window == sequence }
  end
end
