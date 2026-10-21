# Android 发布入口

独立的 **Build Android APK** workflow 已移除。Android 使用
[Release Android to Google Play](../../.github/workflows/release-android-play.yml)，在 GitHub
runner 上通过 Gradle 构建签名 AAB，可选择只构建、上传内部测试草稿或发布内部测试。
它不依赖 EAS 云构建或 Expo 账号。

签名 Secrets、Play 服务账号、版本号迁移、运行命令与验收步骤见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。设计见
[ADR-175](../decisions/clients/adr-175-android-play-gradle-release.md)，取代
[ADR-172](../decisions/clients/adr-172-android-gradle-ci-build.md) 的独立 APK 入口。

桌面 `release.yml` 不再提供 `build_android` 选项，也不会构建新的 Android APK。
重跑旧 Release 时仍校验并保留此前已经发布的 APK 附件，保持其校验和完整。

若需要站外分发 APK，可在上传 AAB 后从 Play Console 下载 Google 签名的通用 APK。
AAB artifact 本身不能直接安装；本 workflow 不会自动下载或发布 APK。
