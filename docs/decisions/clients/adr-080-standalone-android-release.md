# ADR-080：Android APK 独立手动发布

状态：Accepted（2026-10-04）

## 背景

[ADR-078](adr-078-optional-android-release.md) 让 Android 成为桌面发布的手动选项，
另设测试 APK 工作流。两个入口最终调用相同的 EAS 构建，维护者希望像 iOS TestFlight
一样独立触发 Android，并让测试与正式发布复用同一工作流。

## 决策

- `release.yml` 只构建和发布 Linux、macOS。Android 构建失败不影响桌面发布。
- `release-android.yml` 是唯一的 Android APK GitHub Actions 入口，手动选择 `test` 或
  `release`。两种模式共用源码规划、EAS `production-apk` 构建、APK 下载和验证。
- `test` 从所选分支的触发提交构建，创建或更新由版本与提交号命名的独立预发布版本。
- `release` 只能从 `main` 触发，使用已有稳定标签的源码，要求对应的 GitHub Release
  已公开。工作流先验证原 Release 的校验和，再加入 APK，校验完整附件并更新 APK 与
  `SHA256SUMS`。桌面附件和其 `BUILD-INFO.json` 不变。
- 桌面发布重跑时，如果既有 Release 含 Android APK，须先验证原校验和并保留 APK，
  再为新的完整附件集生成校验和。
- Android 密钥仍由 EAS 托管，工作流通过仓库级 `EXPO_TOKEN` 调用 EAS。

## 后果

桌面 Release 完成后可按需追加 Android APK，重跑 Android 不需要重跑桌面构建。
测试和正式模式不再维护两套 Android 工作流。Android APK 的操作步骤见
[Android APK 发布](../../operations/android-releases.md)。
