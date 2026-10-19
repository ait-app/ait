# Android APK 构建与已有产物

独立的 GitHub Actions APK 构建与发布流水线已移除。仓库不再提供生成测试 APK 预发布
或向稳定版追加 APK 的工作流入口。

Android 商店构建继续通过 [Release Android to Google Play](../../.github/workflows/release-android-play.yml)
生成 AAB，支持只构建、上传内部测试草稿和发布内部测试。配置与操作步骤见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。

## 保留的 APK 配置

[`apps/mobile/eas.json`](../../apps/mobile/eas.json) 中的 `production-apk` profile
仍可用于按需调用 EAS CLI 构建 APK；移除 GitHub Actions 入口不改变 profile、项目身份
或 EAS 托管签名。APK 构建使用本地版本号，Google Play 工作流使用远端递增的
`versionCode`。

[`scripts/android-release.mjs`](../../scripts/android-release.mjs) 保留 APK 验证与收集工具，
其中的版本及包名规划也被 Google Play AAB 发布工具复用。

## 已发布的 APK

历史 GitHub Release 中的 APK 不因流水线移除而删除。桌面发布重跑时，仍会验证原有
`SHA256SUMS`、保留已有 APK，并为完整附件集重新生成校验和。

下载已有 Release 的全部附件后，可在对应目录验证：

```bash
sha256sum --check SHA256SUMS
```

生产包名为 `dev.ait.mobile`。安装包签名由原构建所用的 keystore 决定；不同证书签名的
同包名 APK 不能直接互相覆盖。
