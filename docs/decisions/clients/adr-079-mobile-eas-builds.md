# ADR-079：移动端统一使用 Expo EAS 构建

状态：Accepted（2026-10-03）

## 背景

iOS 已通过 EAS 云构建并提交 TestFlight，Android APK 则在自托管 runner 执行 Gradle。
维护者要求 Android 采用类似 iOS 的 EAS 工作流，沿用原有 `production-apk` 配置。

## 决策

- Android 使用 `production-apk`，将该 profile 的 Android 构建资源设为 `medium`，
  适配团队的 Free 套餐。该 profile 的 Android 环境设置 `AIT_ANDROID_HERMES_O0=1`，
  由 Expo config plugin 将 Hermes 编译参数设为 `-O0 -output-source-map`，
  尝试降低生成协议校验代码的编译内存峰值。保留 npm 构建命令。
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

不再需要 Android 自托管 runner。团队新项目使用 EAS 托管的新 Android keystore，
在本地 CLI 完成一次配置后，CI 冻结并复用该凭据。切换签名证书会影响旧签名应用的覆盖安装。
实际云端编译以工作流运行结果为准，操作步骤见 [Android APK 发布](../../operations/android-releases.md)。

Hermes `-O0` 关闭字节码编译优化，保留 Hermes 引擎与 source map。此设置只用于
`production-apk` 的 Android 构建，其他 profile 与 iOS 沿用默认优化。
它是针对 medium 上 `hermesc` 退出 137 的验证性调整；成功构建后仍需检查启动、协议连接
和终端交互的运行性能。后续拆分生成校验代码后，应重新评估恢复默认 `-O`。
