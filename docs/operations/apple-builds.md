# Apple 本地构建

在仓库根目录执行命令。需要 macOS、完整 Xcode（含 iOS SDK）、CocoaPods、Node/npm 和
Rust stable；先运行 `npm ci`。首次构建会下载 Electron、Pods 和 React Native 预编译依赖。
原生工程、签名文件和产物不提交到 Git。建议预留至少 20 GB 磁盘空间。
iOS 最低版本为 16.0，与当前 Skia 二进制依赖一致。

## macOS DMG

```sh
npm run build:dmg
# 同时验证内置 Rust daemon 的启动、鉴权 RPC、重启重连和退出清理：
AIT_DESKTOP_SMOKE=1 npm run build:dmg
```

流程：共享 TypeScript 包 → Electron 专用 Web 导出 → Rust `daemon` release binary →
Electron 主进程 → electron-builder DMG。当前命令只构建宿主架构：Apple Silicon 是 arm64，
Intel 是 x64。Rust binary 和 Electron 必须同架构，不支持直接生成 universal 包。
`AIT_SERVER_BIN` 可指定已有的同架构 binary；默认从当前仓库编译。

输出：`apps/desktop/release/Ait-<version>-local-<arch>.dmg`，其中包含 `Ait.app`。
Rust daemon 位于 App 的 `Contents/Resources/bin/daemon`，Web 页面位于 `app-dist`。
本地构建使用 ad-hoc 签名，关闭 notarization 和上游自动更新源；适合本机验证，不等同于
通过 Gatekeeper 公证的正式分发包。构建始终传入 `--publish never`。

正式分发使用 Developer ID Application 证书和 Apple 公证凭据，例如已保存在钥匙串中的
notarytool profile：

```sh
export CSC_NAME='YOUR NAME (TEAMID)'
export APPLE_KEYCHAIN_PROFILE='your-notary-profile'
npm run build:dmg:release
```

也支持 electron-builder 的 `CSC_LINK` 和 `CSC_KEY_PASSWORD`，以及完整的 Apple ID 或
App Store Connect API key 公证环境变量。正式命令会提交给 Apple 公证服务，不上传到 GitHub。
`CSC_NAME` 填写证书名称中冒号之后的部分，不包含 `Developer ID Application:` 前缀。
它使用 `electron-builder.yml` 的发布配置，更新源已指向 `necokeine/ait`。
Rust 可执行文件也列入签名范围。本机钥匙串 profile 不会自动同步到 GitHub；CI 的签名与
公证 Secrets 见 [发布操作指南](releasing.md)。

## iOS 模拟器 App

移动端默认应用 ID 为 `dev.ait.mobile`，`APP_VARIANT=development` 时为
`dev.ait.mobile.debug`，均使用 `ait://`。原生目录由 Expo prebuild 更新；旧 Paseo
应用不被覆盖。正式签名、推送及 Firebase 配置须对应新的应用 ID；需要自定义 iOS ID
时使用 `IOS_BUNDLE_IDENTIFIER`，不要指向 Paseo 的应用注册。

```sh
npm run build:ios:simulator
```

流程：共享包 → Terminal WebView bundle → Expo prebuild → CocoaPods → Xcode Release build。
不依赖 EAS 云构建，也不需要 Apple 账号或 Metro 开发服务。
默认产物：`apps/mobile/release/ios/simulator/DerivedData/Build/Products/Release-iphonesimulator/Ait.app`（scheme 取自 prebuild 生成的 `.xcworkspace` 名称，脚本结束时会打印实际路径）。

```sh
xcrun simctl boot 'iPhone 17 Pro'  # 使用本机已有的模拟器名称
xcrun simctl install booted apps/mobile/release/ios/simulator/DerivedData/Build/Products/Release-iphonesimulator/Ait.app
xcrun simctl launch booted dev.ait.mobile
```

模拟器 App 不能安装到实体 iPhone。

## iPhone 归档与 IPA

```sh
npm run build:ios
```

生成 arm64 真机 Release 归档：`apps/mobile/release/ios/unsigned/Ait.xcarchive`。
该命令关闭签名，可验证原生编译和 JS 打包，但未经签名的归档不能直接安装或提交 App Store。
手机端只包含客户端，Rust daemon 运行在电脑/服务器上。

导出可安装或可提交的 IPA，需要自己的 Bundle ID、Apple Team、签名证书和匹配的
provisioning profile。在 Xcode 的 Signing & Capabilities 配置自己的 Team，按用途导出一次
ExportOptions.plist（development / ad-hoc / App Store Connect），保存在 Git 之外。
Ait 正式 TestFlight 发布不走这条本地签名路径，而是经由托管在 EAS 的签名凭据和手动 GitHub
Actions 工作流完成，见[发布操作指南的 Apple TestFlight iOS 手动发布一节](releasing.md#apple-testflight-ios-手动发布)
和 [ADR-070](../decisions/clients/adr-070-ios-testflight-release.md)。
本地未签名归档流程仍可用于验证原生编译，然后运行：

```sh
export APPLE_TEAM_ID='YOURTEAMID'
export IOS_BUNDLE_IDENTIFIER='com.yourcompany.ait'
export IOS_EXPORT_OPTIONS_PLIST='/absolute/path/to/ExportOptions.plist'
# 如需 Xcode 联系 Apple 更新 provisioning profile，可显式启用：
# export IOS_ALLOW_PROVISIONING_UPDATES=1
npm run build:ios:ipa
```

产物在 `apps/mobile/release/ios/signed/`。ExportOptions 的 destination 应为 `export`，
这个步骤只导出 IPA；TestFlight/App Store 上传需单独执行。Development/ad-hoc 签名需要
包含目标设备；App Store 签名产物通过 TestFlight/App Store 安装。证书、profile、Apple
密码和 API key 不得提交到仓库。

`IOS_BUILD_JOBS` 默认 4，Metro 默认 2 个 worker，可降低以控制内存；
`EXTRA_PACKAGER_ARGS` 可覆盖 Metro 参数；`APP_VARIANT=development` 生成 Ait Debug。
脚本每次重新应用 Expo 配置，不使用 `prebuild --clean`。原生目录是生成物，请把持久配置
放在 `app.config.js` 或 Expo config plugin 中。

如选择 EAS，先设置自己的 `EXPO_OWNER`、`EAS_PROJECT_ID` 和 `IOS_BUNDLE_IDENTIFIER`，
再从 `apps/mobile` 运行 `eas build --platform ios --profile simulator|preview|production`。
项目不再默认绑定上游 Expo project 或 App Store app。EAS 会使用账号服务及对应签名流程。

## daemon 连接范围

桌面包自动启动内置 Rust daemon。iOS 模拟器可连接宿主 `127.0.0.1:7316`，填写 daemon 的
访问令牌。实体 iPhone 的 `127.0.0.1` 指向手机自身；当前 Rust daemon 只接受 loopback
监听，真机访问电脑仍需要另行配置网络入口/安全隧道。本次打包不改变服务端网络边界。

参考：[electron-builder v26 macOS 配置](https://www.electron.build/v26/docs/mac/)、
[Expo 本地 Release 构建](https://docs.expo.dev/guides/local-app-production/)、
[Apple 注册设备分发](https://developer.apple.com/documentation/xcode/distributing-your-app-to-registered-devices)。
