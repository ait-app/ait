# Android APK 发布

[Release Ait](../../.github/workflows/release.yml) 默认只发布 Linux、macOS。手动选择 Android 后，
才调用 [Android APK 构建工作流](../../.github/workflows/release-android.yml)，与桌面并行构建，
全部成功后一起上传到 GitHub Release。可选发布边界见
[ADR-078](../decisions/clients/adr-078-optional-android-release.md)，EAS 构建方式见
[ADR-079](../decisions/clients/adr-079-mobile-eas-builds.md)。

## 触发发布

按[发布指南](releasing.md#创建-release)推送稳定标签 `vX.Y.Z`，只会自动发布桌面。
需要 Android APK 时，手动运行 **Release Ait**，填写已有标签并勾选
**Also build and publish Android APKs**；该选项默认关闭。CLI 等价命令：

```bash
gh workflow run release.yml --repo OWNER/REPO --ref main -f tag=vX.Y.Z -f build_android=true
```

`source_commit` 继续使用主发布流程的语义：仅在明确需要同版本重构建时指定完整的 40 位
小写提交 SHA，源码版本必须与标签一致；所有平台使用同一个源码选择。

## 手动发布当前 commit 的测试 APK

工作流合入默认分支后，打开 Actions → **Release Android Test APK** → **Run workflow**，
选择要测试的分支直接运行，不需要填写版本标签。工作流固定使用触发时所选分支的 commit。

构建完成后自动创建 GitHub 预发布版本，标签为
`android-test-v版本-12位commit`，例如 `android-test-v0.0.15-124bc2e1c6f9`。
该版本包含通用 APK、`SHA256SUMS` 和记录实际 commit 的 `BUILD-INFO.json`，
不会成为最新正式 Release。同一 commit 重跑会更新同一预发布版本的附件。

```bash
gh workflow run release-android-test.yml --repo OWNER/REPO --ref BRANCH
```

## APK 与签名

沿用原有 `production-apk` EAS profile，产物为一个通用 APK。例如 `v0.0.15` 发布为
`Ait-0.0.15-android.apk`，同时支持 ARM64 和 ARMv7。生产包名为 `dev.ait.mobile`，
最低系统版本随 `apps/mobile/app.config.js` 配置，当前为 Android 10。

APK 使用原 profile 默认的 EAS 托管签名。首次 CI 构建前须在 EAS 配置 Android keystore；
工作流冻结凭据，不创建或更换密钥，也不重新签名下载的 APK。
原测试签名安装包不能直接被不同证书签名的同包名 APK 覆盖。
本流程提供 GitHub APK 下载，Google Play 内测仍按原有发布指南执行。

Android APK 与桌面附件共用 Release 中的 `SHA256SUMS` 和 `BUILD-INFO.json`。
前者包含 APK 的 SHA-256，后者记录版本、实际源码 SHA、工作流 SHA 和运行链接。
下载全部附件到同一目录后可运行：

```bash
sha256sum --check SHA256SUMS
```

## 构建与重试

Android 和 iOS 都使用 Expo EAS 云构建。Android 直接使用现有 `production-apk` profile，
iOS 继续使用 `ait`。`eas.json` 中的继承关系、Node 版本、Gradle 命令和原生配置保持原样；
工作流按 iOS 的方式从 `build.ait.env` 加载 Ait 项目身份。

当前 EAS 项目 ID 为 `379ada50-82c0-4d4a-bac9-cb8c113cf38d`。`app.config.js` 也从
`build.ait.env` 读取默认的 owner、slug 和 project ID，因此本地 CLI 与云端 APK 构建使用
同一项目；显式设置的同名环境变量仍可覆盖这些公开标识。

在仓库 Settings → Secrets and variables → Actions 配置 `EXPO_TOKEN`，令牌对应的 Expo
账号须有 `sd542927172s-team/ait` 项目的构建权限。正式发布和测试发布都将此 Secret 传入
Android 可复用工作流。若原来的令牌只放在 `ios-testflight` Environment 中，还需要配置
仓库级 Secret。原 `production-apk` 使用 `large` 资源，需要支持该资源的 Expo 套餐。

GitHub 托管 Ubuntu runner 触发 EAS、等待结果并下载 APK，仅安装 Android Build Tools
用于验证。EAS 执行依赖安装、共享 UI 和终端 WebView 构建、Expo prebuild 与 Gradle 编译。
无需自托管 runner。参考 Expo 的 [CI 构建说明](https://docs.expo.dev/build/building-on-ci/)和
[EAS 配置说明](https://docs.expo.dev/eas/json/)。

APK 通过签名、16 KB 对齐、包信息和架构检查后，以 `ait-android-apk` artifact 保存 7 天。
选中 Android 的主发布任务等待桌面和 Android 全部成功，再校验附件并创建或更新 Release。
未选中时只校验桌面附件。

Android 工作流等待 EAS 最多 120 分钟；超时或取消 GitHub 任务后，可在 Expo 控制台查看和
取消仍在进行的构建。失败时可重跑失败任务，重跑会创建新的 EAS 构建。
