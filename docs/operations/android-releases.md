# Android APK 发布

[Release Ait](../../.github/workflows/release.yml) 自动调用
[Android APK 构建工作流](../../.github/workflows/release-android.yml)，与 Linux、macOS 并行构建，
全部成功后一起上传到 GitHub Release。发布职责见
[ADR-077](../decisions/clients/adr-077-android-apk-release.md)。

## 触发发布

将工作流合入发布源码后，按[发布指南](releasing.md#创建-release)推送稳定标签 `vX.Y.Z`，
即会自动构建 Android APK，无需再单独运行 Android Action，也不要求事先创建 GitHub Release。
现有 **Release Ait** 的手动发布入口同样包含 Android：

```bash
gh workflow run release.yml --repo OWNER/REPO --ref main -f tag=vX.Y.Z
```

`source_commit` 继续使用主发布流程的语义：仅在明确需要同版本重构建时指定完整的 40 位
小写提交 SHA，源码版本必须与标签一致；所有平台使用同一个源码选择。

## 手动发布当前 commit 的测试 APK

工作流合入默认分支后，打开 Actions → **Release Android Test APK** → **Run workflow**，
选择要测试的分支即可，不需要填写版本标签。工作流固定使用触发时所选分支的 commit；
随后分支有新提交也不会改变本次构建源码。

构建完成后自动创建 GitHub 预发布版本，标签为
`android-test-v版本-12位commit`，例如 `android-test-v0.0.14-124bc2e1c6f9`。
该版本包含两个标准文件名的 APK、`SHA256SUMS` 和记录实际 commit 的 `BUILD-INFO.json`，
不会成为最新正式 Release。同一 commit 重跑会更新同一预发布版本的附件。

```bash
gh workflow run release-android-test.yml --repo OWNER/REPO --ref BRANCH
```

## APK 与命名

APK 沿用现有安装包的 `Ait-版本-平台-架构.扩展名` 格式。例如 `v0.0.14` 发布包含：

| 附件                           | 适用架构                                |
| ------------------------------ | --------------------------------------- |
| `Ait-0.0.14-android-arm64.apk` | ARM64，Android ABI 为 `arm64-v8a`       |
| `Ait-0.0.14-android-armv7.apk` | 32 位 ARM，Android ABI 为 `armeabi-v7a` |

两个文件都是可独立安装的完整 APK，按设备架构选择一个即可。生产包名为 `dev.ait.mobile`，
最低系统版本随 `apps/mobile/app.config.js` 配置，当前为 Android 10。

Android APK 与桌面附件共用 Release 中的 `SHA256SUMS` 和 `BUILD-INFO.json`。
前者包含每个 APK 的 SHA-256，后者记录所有平台的版本、实际源码 SHA、工作流 SHA 和运行链接。
下载全部附件到同一目录后可运行：

```bash
sha256sum --check SHA256SUMS
```

## 当前签名方式

暂时跳过正式签名配置，保留 Expo 生成项目中 `assembleRelease` 使用的测试 keystore，
无需配置 Android 签名 Secrets。这仍是内置 JavaScript bundle、不可调试的 release 构建；
测试签名使 APK 可以安装。工作流验证现有签名、包名、版本和各 APK 的架构，不进行重新签名。

后续切换正式密钥时，原测试签名安装包不能直接被不同证书签名的同包名 APK 覆盖。
本流程提供 GitHub APK 下载，Google Play 内测仍按原有发布指南执行。

## 构建与重试

runner 安装 Android SDK 后，构建共享依赖和终端 WebView，通过 Expo prebuild 生成原生项目，
为两种 ARM ABI 配置独立 APK，再执行 Gradle release 构建。生成的 Android 项目只用于本次构建。

两个 APK 均通过签名、16 KB 原生库对齐、包信息与架构检查后，以 `ait-android-apk` artifact
保存 7 天。主发布任务等待桌面和 Android 构建全部成功，再合并附件、验证完整性、生成校验和，
最后创建或更新 GitHub Release。失败时可在该次 **Release Ait** 运行中重跑失败任务。

ABI 分包机制参见 Android 官方的[多 APK 构建说明](https://developer.android.com/build/configure-apk-splits)。
