# ADR-172：Android APK 在 GitHub runner 上用 Gradle 构建

- 状态：Accepted
- 日期：2026-10-20

## 背景

[ADR-080](adr-080-standalone-android-release.md) 的 EAS APK 工作流已移除，之后 APK 只能在
维护者本地构建。本地流程是 `npm ci`、`build:ui-deps`、`expo prebuild` 与
`gradlew :app:assembleRelease`，不依赖 EAS，可以直接在 GitHub 托管 runner 上执行，
不消耗 EAS 构建配额。

## 决策

- 新增可复用工作流 `build-android.yml`，在 `ubuntu-24.04` 上按本地步骤构建通用 APK，
  沿用 `production-apk` 的 `APP_VARIANT=production`、`AIT_ANDROID_HERMES_O0=1` 和
  跳过 lint 的 Gradle 参数。产物经 `scripts/android-release.mjs` 验证签名、16 KB 对齐、
  包名、版本与 ARM 架构后，以 `ait-android-apk` artifact 保存 14 天。
- 手动运行（`workflow_dispatch`）构建所选分支，版本取 `apps/mobile/package.json`。
- `release.yml` 默认只构建桌面；明确要求 Android 时，通过手动入口开启 `build_android`
  并指定稳定标签，APK 随桌面附件进入同一 Release 和 `SHA256SUMS`。beta 标签不构建 APK。
  Android 失败不阻止桌面发布，此时保留 Release
  中已有的 APK。
- 签名使用仓库 Secrets 中的 keystore，通过 AGP 的 `android.injected.signing.*`
  属性覆盖 prebuild 生成的 debug 签名配置。标签构建缺少 Secrets 时失败；
  手动构建退回 debug keystore 并给出警告。
- Google Play AAB 仍走 EAS 与 Play App Signing，不受影响。

## 后果

APK 构建不再依赖 EAS 账号与 `EXPO_TOKEN`。为与此前 GitHub Release 中由 EAS 签名的
APK 保持覆盖安装，Secrets 应使用从 EAS 导出的同一 keystore。Release 工作流需等待
Android 构建完成，整体耗时随之增加。操作步骤见
[Android APK 构建](../../operations/android-releases.md)。
