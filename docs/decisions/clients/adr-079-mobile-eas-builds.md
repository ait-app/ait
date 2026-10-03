# ADR-079：移动端统一使用 Expo EAS 构建

状态：Accepted（2026-10-03）

## 背景

iOS 已通过 EAS 云构建并提交 TestFlight，Android APK 则在自托管 runner 执行 Gradle。
维护者要求 Android 采用类似 iOS 的 EAS 工作流，沿用原有 `production-apk` 配置。

## 决策

- 保留现有 `eas.json`、Expo 原生配置和 npm 构建命令。Android 使用 `production-apk`，
  iOS 继续使用 `ait`，工作流按 iOS 的方式加载 Ait EAS 项目身份。
- Android 工作流使用 GitHub 托管 Ubuntu runner 发起 EAS 构建并等待结果。编译、架构和
  签名遵循现有 EAS profile，不额外注入 Gradle 参数或分包插件。
- 工作流下载 EAS 返回的通用 APK，验证版本、包名、签名、ARM 架构和 16 KB 对齐，
  以 `Ait-版本-android.apk` 发布到 GitHub Release。
- [ADR-078](adr-078-optional-android-release.md) 的手动可选发布继续有效；通用 APK 和
  EAS 管理的签名替代 [ADR-077](adr-077-android-apk-release.md) 的双 APK 与测试签名。
- 正式和测试入口显式传递仓库级 `EXPO_TOKEN`；iOS 继续遵循
  [ADR-070](adr-070-ios-testflight-release.md)，手动提交 TestFlight。

## 后果

不再需要 Android 自托管 runner。EAS 项目需要配置构建凭据，并具备原 profile 要求的
`large` 构建资源。切换签名证书会影响旧测试签名应用的覆盖安装。
实际云端编译以工作流运行结果为准，操作步骤见 [Android APK 发布](../../operations/android-releases.md)。
