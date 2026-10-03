# ADR-078：Android 发布改为手动可选

状态：Accepted（2026-10-03）

## 背景

[ADR-077](adr-077-android-apk-release.md) 使标签发布自动运行 Android 构建，并要求 APK
齐全后才能发布桌面。维护者要求 Android 默认不运行，按需手动启用。

## 决策

- `release.yml` 的 `workflow_dispatch` 增加布尔输入 `build_android`，默认 `false`。
  只有手动运行并显式选中该输入才调用 Android 工作流；推送标签仅构建 Linux、macOS。
- 主发布 job 显式允许 Android 被跳过，但仍要求桌面构建成功且运行未被取消。
  选中 Android 时，必须等待其成功，失败、取消或跳过均不发布。
- 资产校验默认只接受完整桌面资产。只有 `verify VERSION DIRECTORY --android` 才要求
  两个 Android APK，并将它们纳入同一校验和；默认模式拒绝意外的 APK，避免误发布。
- `release-android-test.yml` 保持独立的纯手动测试入口；APK 身份、架构、测试签名与
  原生库校验继续遵循 ADR-077。

## 后果

日常推送标签不会消耗 Android 构建资源，也不会等待 Android。需要 APK 时，维护者在
Release Ait 手动入口选择已有标签并勾选 Android，或者使用独立 Android 测试发布。
操作步骤见 [Android APK 发布](../../operations/android-releases.md)。
