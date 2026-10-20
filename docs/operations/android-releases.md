# Android APK 构建

[Build Android APK](../../.github/workflows/build-android.yml) 在 GitHub 托管的
Ubuntu runner 上直接用 Gradle 构建通用 APK，不调用 EAS。设计见
[ADR-172](../decisions/clients/adr-172-android-gradle-ci-build.md)。

Android 商店构建继续通过 [Release Android to Google Play](../../.github/workflows/release-android-play.yml)
生成 AAB，支持只构建、上传内部测试草稿和发布内部测试。配置与操作步骤见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。

## 手动构建

打开 Actions → **Build Android APK** → **Run workflow**，选择要构建的分支。工作流使用该分支
最新提交，版本取 `apps/mobile/package.json`，APK 以 `ait-android-apk` artifact 保存 14 天，
不创建 Release。CLI 等价命令：

```bash
gh workflow run build-android.yml --repo OWNER/REPO --ref BRANCH
```

## 随稳定版发布

推送稳定标签 `vX.Y.Z` 默认只构建桌面。明确要求本次包含 Android APK 时，手动运行
[Release Ait](../../.github/workflows/release.yml)，指定标签并开启 `build_android`；工作流与桌面
并行构建 `Ait-VERSION-android.apk`，加入草稿 Release 并写入 `SHA256SUMS`。例如：

```bash
gh workflow run release.yml --repo OWNER/REPO --ref main -f tag=vX.Y.Z -F build_android=true
```

beta 标签始终不构建 APK。Android 构建失败不阻止桌面发布：Release 若已有 APK 则校验后保留，
并在运行日志中给出警告。修复后需显式开启同一选项重跑；Google Play 与 iOS 仍使用独立发布入口。

## 构建步骤

与本地构建一致：`npm ci` → `npm run build:ui-deps` → `apps/mobile` 中
`expo prebuild --platform android --clean` → `gradlew :app:assembleRelease`（跳过 lint 任务）。
环境变量沿用 `production-apk` profile：`APP_VARIANT=production`、`AIT_ANDROID_HERMES_O0=1`，
后者让 Hermes 以 `-O0` 编译以降低内存开销。APK 包含 ARM64、ARMv7、x86 与 x86_64。

APK 通过签名、16 KB 对齐、包信息、非 debuggable 与 ARM 架构检查后才会上传；运行摘要记录
源码提交、签名方式和证书 SHA-256。

## 签名

在仓库 Settings → Secrets and variables → Actions 配置：

| Secret                      | 内容                        |
| --------------------------- | --------------------------- |
| `ANDROID_KEYSTORE_BASE64`   | keystore 文件的 base64      |
| `ANDROID_KEYSTORE_PASSWORD` | keystore 密码               |
| `ANDROID_KEY_ALIAS`         | 签名 key 的别名             |
| `ANDROID_KEY_PASSWORD`      | key 密码（PKCS12 同上一项） |

此前 GitHub Release 中的 APK 由 EAS 托管 keystore 签名。要让用户覆盖安装，应导出同一
keystore：在已登录 Expo 的终端执行 `cd apps/mobile && eas credentials --platform android`，
选择 `production-apk` 所用凭据并下载 keystore，再用 `base64 -w0 keystore.jks` 写入 Secret。
密钥不进入仓库。

标签构建缺少任一 Secret 时失败。手动构建会退回 Gradle 的 debug keystore 并给出警告，
这类 APK 只适合测试，不能覆盖安装正式签名的同包名应用。Google Play 安装的应用使用
Play App Signing 证书，与上述两种 APK 都不同。

## 校验

下载 Release 的全部附件后，可在对应目录验证：

```bash
sha256sum --check SHA256SUMS
```

生产包名为 `dev.ait.mobile`，最低系统版本为 Android 10。
