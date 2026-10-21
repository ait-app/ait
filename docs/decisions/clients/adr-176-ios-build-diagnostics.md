# ADR-176：iOS 构建诊断按依赖与自有代码分层

- 状态：Accepted
- 日期：2026-10-21

## 背景

Xcode 27 的 Cloud Build 记录 558 条 warning，主要是 React umbrella header 的
`incomplete-umbrella` 与 `nullability-completeness` 诊断、第三方静态库空目标文件的 libtool
警告，以及 Expo / React Native 头文件在 Swift 导入时的协议声明提示。应用自己的废弃 API
提示被这些依赖诊断掩盖，构建日志失去可读性；修复又不能在 `apps/mobile/ios` 手改，因为原生
目录是 [Expo prebuild 生成物](../../operations/apple-builds.md)。

信息来源分三类，处理方式也必须分开：固定版本的第三方 Pods、Ait 自有原生模块、以及只有
Swift Clang importer 才会触发的上游头文件诊断。

## 决策

新增 `with-ios-build-diagnostics` 配置插件，在 Podfile 中加载
`apps/mobile/plugins/ios/build_diagnostics.rb`，本地 Xcode 与 Xcode Cloud 共用同一份配置。

- 目标定义默认 `inhibit_all_warnings = true`，逐个把自有 Pod（`OWNED_PODS`）重新打开；
  第三方源码警告静音，Ait 与自有的 `ExpoTwoWayAudio`、`PaseoHardwareKeyboard` 保持诊断。
- 第三方静态库追加 libtool 的 `-no_warning_for_no_symbols`，覆盖按平台排除或代码生成的
  空目标文件；自有 Pod 不追加。
- 仅向应用的 Swift Clang importer 传入 `-Wno-incomplete-umbrella` 与
  `-Wno-nullability-completeness` 两个例外，Swift 与 ObjC 源码警告继续显示。
- Hermes 版本选择脚本设为 `always_out_of_date`，配置切换时仍选择正确二进制，同时消除缺少
  输出文件的脚本阶段警告。
- CocoaPods 生成的 Swift 库搜索路径从 `TOOLCHAIN_DIR` 改为 `SDKROOT`，避免 Xcode Cloud
  选中 Metal toolchain 后去不存在的目录查找 Swift 库。
- 移除冷启动 URL 的 `UIApplication.LaunchOptionsKey.annotation`（iOS 16 起废弃且已不再
  使用），音频模块改用等价的 `allowBluetoothHFP` 名称并显式忽略停止录音的返回值。

## 后果

依赖警告静音后，上游源码问题不再出现在构建日志中，只能随依赖升级复查；升级 Expo /
React Native 后需要重新评估这两个 Swift importer 例外是否仍然必要。

`OWNED_PODS` 是硬编码清单，新增自有原生 Pod 必须同步，回归测试会把它与仓库内的
`*.podspec` 名称逐一比对，不一致即失败。

搜索路径改写只覆盖聚合 xcconfig（当前静态链接配置的链接入口）。启用
`ios.useFrameworks` / `USE_FRAMEWORKS` 前还要处理 Pod targets 与 CocoaPods 的 embed
frameworks 脚本。改写命中 0 次时 `post_install` 会打印 `Pod::UI` 警告，避免 CocoaPods 升级
改变生成格式后这条修复静默失效。

Xcode Cloud 不运行 prebuild，因此提交的 `Ait.xcodeproj/project.pbxproj` 与
`Ait/AppDelegate.swift` 也是配置的一部分。回归测试直接比对两者的内容与插件产物，避免提交件
与插件漂移。

## 验证

`npm run test:mobile-release` 覆盖 Expo 模板、插件组合与重复执行、真实 Ruby 配置处理、
自有模块诊断保留、已有编译参数保留、冷启动链接、提交工程与提交 AppDelegate 的一致性。

原生验证使用 Xcode 27 的 Debug / arm64 未签名构建：空 DerivedData 完整构建返回
`BUILD SUCCEEDED`，最终 xcresult 为 0 errors / 3 warnings，剩余 3 条是 Expo 开发启动器头文件
中 `RCTComponentViewFactoryComponentProvider`、`RCTHostDelegate`、
`RCTTurboModuleManagerDelegate` 的协议声明提示，等待上游修复。严格模式
`bundle exec pod install --deployment` 成功（125 Pods）。

## 参考

- [Apple：`UIApplication.LaunchOptionsKey.annotation`](https://developer.apple.com/documentation/uikit/uiapplication/launchoptionskey/annotation)
- [Apple：`AVAudioSession.CategoryOptions.allowBluetoothHFP`](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetoothhfp)
- [CocoaPods：抑制 Pod 警告](https://guides.cocoapods.org/syntax/podfile.html#inhibit_all_warnings!)
