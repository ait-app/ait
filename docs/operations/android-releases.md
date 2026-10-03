# Android APK 发布

[Release Android APK](../../.github/workflows/release-android.yml) 是独立的手动发布入口，
与桌面 [Release Ait](../../.github/workflows/release.yml) 分开运行。测试和正式模式共用 EAS
构建、APK 验证。发布边界见 [ADR-080](../decisions/clients/adr-080-standalone-android-release.md)，
EAS 构建方式见 [ADR-079](../decisions/clients/adr-079-mobile-eas-builds.md)。

## 触发发布

先按[发布指南](releasing.md#创建-release)推送稳定标签 `vX.Y.Z`，等待桌面 GitHub Release
发布完成。再在 `main` 手动运行 **Release Android APK**，选择 `release` 并填写同一标签。
工作流要求已有非草稿、非预发布的 Release，下载并核对原有附件的 `SHA256SUMS`，
加入 APK 后重新校验完整资产并上传 APK 与更新后的校验和。CLI 等价命令：

```bash
gh workflow run release-android.yml --repo OWNER/REPO --ref main -f mode=release -f tag=vX.Y.Z
```

正式 Android 构建固定使用标签对应的提交。重跑同一标签会重新构建 APK，并覆盖该 Release
中同名的 APK 与校验和；桌面附件和原有 `BUILD-INFO.json` 保持不变。之后若重跑桌面发布，
工作流会核对并保留已发布的 APK，重新生成包含它的校验和。

## 手动发布当前 commit 的测试 APK

打开 Actions → **Release Android APK** → **Run workflow**，选择要测试的分支，保留默认
`test` 模式和空标签。工作流固定使用触发时所选分支的 commit。

构建完成后自动创建 GitHub 预发布版本，标签为
`android-test-v版本-12位commit`，例如 `android-test-v0.0.15-124bc2e1c6f9`。
该版本包含通用 APK、`SHA256SUMS` 和记录实际 commit 的 `BUILD-INFO.json`，
不会成为最新正式 Release。同一 commit 重跑会更新同一预发布版本的附件。

```bash
gh workflow run release-android.yml --repo OWNER/REPO --ref BRANCH -f mode=test
```

## APK 与签名

沿用原有 `production-apk` EAS profile，产物为一个通用 APK。例如 `v0.0.15` 发布为
`Ait-0.0.15-android.apk`，同时支持 ARM64 和 ARMv7。生产包名为 `dev.ait.mobile`，
最低系统版本随 `apps/mobile/app.config.js` 配置，当前为 Android 10。

APK 使用原 profile 默认的 EAS 托管签名。首次 CI 构建前须在 EAS 配置 Android keystore；
工作流冻结凭据，不创建或更换密钥，也不重新签名下载的 APK。
原测试签名安装包不能直接被不同证书签名的同包名 APK 覆盖。
本流程提供 GitHub APK 下载，Google Play 内测仍按原有发布指南执行。

首次配置签名时，在已登录 Expo 的本地终端执行：

```bash
cd apps/mobile
eas credentials:configure-build --platform android --profile production-apk
```

团队新项目使用新建的 Android keystore，由 EAS 托管并供后续构建复用。密钥不进入仓库。

正式 Android APK 加入已有桌面 Release，并更新覆盖全部附件的 `SHA256SUMS`；原有
`BUILD-INFO.json` 记录桌面构建来源。测试预发布单独包含 APK、`SHA256SUMS` 和记录
Android 构建来源的 `BUILD-INFO.json`。
下载全部附件到同一目录后可运行：

```bash
sha256sum --check SHA256SUMS
```

## 构建与重试

Android 和 iOS 都使用 Expo EAS 云构建。Android 直接使用现有 `production-apk` profile，
iOS 继续使用 `ait`。保留 `eas.json` 中的继承关系、Node 版本和 Gradle 命令；
工作流按 iOS 的方式从 `build.ait.env` 加载 Ait 项目身份。

当前 EAS 项目 ID 为 `379ada50-82c0-4d4a-bac9-cb8c113cf38d`。`app.config.js` 也从
`build.ait.env` 读取默认的 owner、slug 和 project ID，因此本地 CLI 与云端 APK 构建使用
同一项目；显式设置的同名环境变量仍可覆盖这些公开标识。

在仓库 Settings → Secrets and variables → Actions 配置 `EXPO_TOKEN`，令牌对应的 Expo
账号须有 `sd542927172s-team/ait` 项目的构建权限。正式和测试模式都使用此 Secret。
若原来的令牌只放在 `ios-testflight` Environment 中，还需要配置
仓库级 Secret。`production-apk` 显式使用 `medium` 构建资源，适配团队当前的 Free 套餐；
APK 类型和 Gradle 命令保持原样。

`production-apk.android.env` 设置 `AIT_ANDROID_HERMES_O0=1`，Expo prebuild 通过
`with-android-hermes-o0` 插件写入 `hermesFlags = ["-O0", "-output-source-map"]`。
这会关闭 Hermes 编译优化，尝试降低生成协议校验代码的内存开销，保留 source map。
其他 profile 和 iOS 不启用此开关。删除该环境变量后，新的干净 prebuild 将恢复默认 `-O`；
本地复现时也需使用同一环境变量。构建成功后需验证启动与交互性能，不能仅以 APK 生成
判断该优化等级适合长期发布。

GitHub 托管 Ubuntu runner 触发 EAS、等待结果并下载 APK，仅安装 Android Build Tools
用于验证。EAS 执行依赖安装、共享 UI 和终端 WebView 构建、Expo prebuild 与 Gradle 编译。
无需自托管 runner。参考 Expo 的 [CI 构建说明](https://docs.expo.dev/build/building-on-ci/)和
[EAS 配置说明](https://docs.expo.dev/eas/json/)。

APK 通过签名、16 KB 对齐、包信息和架构检查后，以 `ait-android-apk` artifact 保存 7 天。
测试模式创建或更新独立的预发布版本；正式模式仅更新指定的既有桌面 Release。

Android 工作流等待 EAS 最多 120 分钟；超时或取消 GitHub 任务后，可在 Expo 控制台查看和
取消仍在进行的构建。失败时可重跑失败任务，重跑会创建新的 EAS 构建。
