# ADR-175：Google Play AAB 在 GitHub runner 上构建和提交

- 状态：Accepted
- 日期：2026-10-20
- 取代：ADR-097 的 EAS 构建、版本与提交机制，以及 ADR-172 的独立 APK 工作流。

Android 商店发布统一使用 `release-android-play.yml`：GitHub 托管 runner 本地执行
`expo prebuild` 和 Gradle `:app:bundleRelease`，使用仓库 Secrets 中的既有上传 keystore
签名，经 bundletool、jarsigner 和 manifest 校验后，通过 Play Developer API 上传到
`internal`。保留 `build-only`、`internal-draft` 和 `internal` 三种手动动作；不再依赖
EAS Build、EAS Submit 或 `EXPO_TOKEN`。Expo SDK 和本地 prebuild 仍属于移动端工具链。

上传 keystore 与服务账号 JSON 归 GitHub Secrets；Google 继续管理 Play App Signing 的
应用签名密钥。版本计数从 EAS 迁至 Actions，使用固定迁移基数加 `run_number × 100 +
run_attempt`，为新运行及重试分配不同的 `versionCode`。迁移基数应覆盖 Play 既有最大值，
每个 run 限制 99 次 attempt，最终版本号遵守 Play 上限；较旧 run 在新版本发布后应改用
新的运行重试。修改仅发生在 runner 生成的 Gradle 配置中。

删除 `build-android.yml`，桌面 Release 移除 Android 构建开关和依赖，但继续验证并保留
已发布 APK 的附件与校验和。iOS/EAS 配置不变。运维步骤见
[Google Play 内部测试与 CI 发布](../../operations/google-play-internal-testing.md)。
