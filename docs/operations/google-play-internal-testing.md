# Google Play 内部测试与 CI 发布

[Release Android to Google Play](../../.github/workflows/release-android-play.yml) 在 GitHub
托管的 Ubuntu runner 上构建签名 AAB，并按选择上传到 Google Play 的内部测试轨道。
包名固定为 `dev.ait.mobile`。构建和提交不调用 EAS，也不需要 `EXPO_TOKEN`。
移动端仍使用 Expo SDK；`expo prebuild` 只在 runner 本地生成 Android 工程。
设计见 [ADR-175](../decisions/clients/adr-175-android-play-gradle-release.md)。

## 从 EAS 迁移

已有 Play 应用、测试者名单和服务账号可继续使用。迁移需要把原有上传 keystore 和
Play 服务账号 JSON 配置到 GitHub Secrets，并设置版本号基数。不要生成新的上传 key
来代替已经在 Play 登记的 key。iOS TestFlight 继续使用 EAS，因此不要删除共享的
`EXPO_TOKEN` 或 EAS 项目凭据。

若原上传 keystore 只保存在 EAS，可在已登录 Expo 的终端一次性导出：

```bash
cd apps/mobile
npx --no-install eas credentials --platform android
```

选择 `dev.ait.mobile` 的现有 production / production-play 凭据，下载 keystore 并记录
alias 和密码。这只是凭据迁移；新 CI 无需登录 Expo。

## GitHub 配置

在仓库 Settings → Secrets and variables → Actions 添加以下 Secrets：

| Secret                             | 内容                                                        |
| ---------------------------------- | ----------------------------------------------------------- |
| `ANDROID_KEYSTORE_BASE64`          | Play 已登记的上传 keystore 文件的 base64                    |
| `ANDROID_KEYSTORE_PASSWORD`        | keystore 密码                                               |
| `ANDROID_KEY_ALIAS`                | 上传 key 的别名                                             |
| `ANDROID_KEY_PASSWORD`             | key 密码；PKCS12 通常与 keystore 密码相同                   |
| `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` | 有 Ait 测试轨道发布权限的服务账号完整 JSON，原文而非 base64 |

例如，在 macOS 上直接把文件传给 GitHub CLI，不将内容写入仓库或终端输出：

```bash
base64 < /absolute/path/upload.keystore | tr -d '\n' | gh secret set ANDROID_KEYSTORE_BASE64 --repo ait-app/ait
gh secret set GOOGLE_PLAY_SERVICE_ACCOUNT_JSON --repo ait-app/ait < /absolute/path/play-service-account.json
```

其余三个 Secrets 可在 GitHub 页面填写，或使用 `gh secret set NAME --repo ait-app/ait`
交互输入。keystore、密码与服务账号 JSON 不进入 Git、日志、聊天或 issue。
只构建 AAB 也必须配置四个签名 Secrets；缺失时失败，不回退 debug 签名。
`GOOGLE_PLAY_SERVICE_ACCOUNT_JSON` 仅上传时需要。

在 **Variables** 中配置 `ANDROID_PLAY_VERSION_CODE_BASE`：迁移时将它设置为
Play 历史最大 `versionCode`，后续保持不变。新应用可以不设置，默认为 `0`。
例如 Play 中已有最大值 `47`：

```bash
gh variable set ANDROID_PLAY_VERSION_CODE_BASE --repo ait-app/ait --body 47
```

版本号计算为：

```text
versionCode = ANDROID_PLAY_VERSION_CODE_BASE + GITHUB_RUN_NUMBER × 100 + GITHUB_RUN_ATTEMPT
```

每次新运行及重跑均分配不同版本号，每个 run 最多支持 99 次 attempt，结果不得超过
Play 的 `2100000000` 上限。`versionName` 保持仓库版本不变。计数只写入 runner 生成的
`android/app/build.gradle`，不修改仓库、iOS 版本或 EAS 远端计数器。

## 首次配置 Play

1. 在 Play Console 创建 Ait 应用，并在内部测试的测试者页面配置名单。首个上传包的包名
   必须为 `dev.ait.mobile`。参见 [内部测试说明](https://support.google.com/googleplay/android-developer/answer/9845334)。
2. 在 Google Cloud 项目启用 **Google Play Android Developer API**，创建或复用发布服务
   账号，并取得 JSON 密钥。无需为此授予 Cloud 项目管理员权限，也无需关联 Play 开发者
   账号与 Cloud 项目。参见 [API 入门](https://developers.google.com/android-publisher/getting_started)。
3. 在 Play Console「用户和权限」邀请服务账号邮箱，只添加 Ait 应用，授予「查看应用信息」
   和「将应用发布到测试轨道」。CI 不维护测试者名单，无需管理员、财务或正式版发布权限。
4. 将服务账号 JSON 配置为上述 GitHub Secret。若旧 JSON 仅保存在 EAS 且无法取回，
   为同一服务账号创建新 JSON key；旧 EAS/iOS 凭据无需变更。
5. 对尚未通过 Console 初始化的应用，先运行 `build-only`，下载 AAB，手动上传至内部测试
   草稿，按提示配置 Play App Signing 和首轮发布。上传 action 要求包名已存在于 Play。
   已有 Play 版本的应用跳过此步。

CI 使用 [upload-google-play](https://github.com/r0adkll/upload-google-play) 直接调用 Play
Developer API，上传轨道固定为 `internal`，没有 production 或 internal sharing 入口。

## 手动运行

打开 Actions → **Release Android to Google Play** → **Run workflow**，选择源码分支：

| action               | 行为                                         |
| -------------------- | -------------------------------------------- |
| `build-only`（默认） | 构建、签名并校验 AAB，只上传 GitHub artifact |
| `internal-draft`     | 构建并上传到 Play，保留内部测试草稿          |
| `internal`           | 构建并以 `completed` 状态提交给内部测试者    |

空 `tag` 使用本次触发时的分支 commit；填写稳定标签 `vX.Y.Z` 时使用该标签源码，并检查
标签版本与 `apps/mobile/package.json` 一致。不会因推送标签、PR 或合并分支自动上传。

```bash
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=build-only
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=internal-draft
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=internal -f tag=vX.Y.Z
```

运行摘要记录源码 commit、版本、实际 `versionCode` 和构建结果。
`ait-android-play-aab` artifact 保留 14 天，包含签名 `.aab`、`BUILD-INFO.json`、
从该 AAB 提取的 `mapping.txt` 和 `SHA256SUMS`。若 AGP 在 AAB 中提供 `r8.json`，也会
提取并纳入校验和；较旧 AGP 缺少此文件时，Play 使用映射文件或 DEX 特征评估优化情况。
下载解压后验证：

```bash
sha256sum --check SHA256SUMS
```

## 构建与签名

runner 执行 `npm ci` → `npm run build:ui-deps` → `expo prebuild --platform android --clean`
→ `gradlew :app:bundleRelease`。使用 Java 17、Node 24、`APP_VARIANT=production` 和
`AIT_ANDROID_HERMES_O0=1`。AGP 的 `android.injected.signing.*` 属性将 release 签名覆盖为
上传 keystore；构建结束始终清理临时 keystore。

提交前使用固定版本及 SHA-256 的 bundletool 检查结构与 manifest，使用 jarsigner 验证
上传签名，核对包名、版本、实际 `versionCode` 和非 debug 属性。校验通过后保留原始 AAB
字节并生成校验和，上传 action 只提交本次通过校验的那一个 AAB。

AAB 使用 **上传密钥** 签名。Google Play 安装包由 **Play App Signing 的应用签名密钥**
签名，两种凭据不同。需要站外分发相同应用签名的 APK 时，在 Play Console 的应用版本
下载页面取得 Google 签名的 universal APK；本 workflow 当前只交付 AAB。

### R8 与 DEX 优化

`apps/mobile/app.config.js` 的 `expo-build-properties.android` 启用
`enableMinifyInReleaseBuilds` 和 `enableShrinkResourcesInReleaseBuilds`。
`with-android-release-optimization` 插件将生成工程的默认规则切换为
`proguard-android-optimize.txt`，保留应用和依赖自身的规则。不要手改生成的 Android 工程，
下次 `expo prebuild --clean` 会重新生成它。

Play CI 在 Gradle 构建前检查这两个开关、R8 full mode 和默认规则。缺少配置的旧标签会
在构建前失败，应使用包含这些配置的源码发布。构建后要求 AAB 自身包含有效的 R8
`BUNDLE-METADATA/com.android.tools.build.obfuscation/proguard.map`；缺失或空映射会阻止
产物收集和 Play 上传。映射随原始 AAB 上传，并在 artifact 中另存，供崩溃还原使用。

PR CI 的 `test:mobile-release` 对安装的 Expo 模板实际运行配置插件，并验证 Play 产物
检查会拒绝缺失或损坏的映射。此检查不能代替 Release 真机验收：首次开启 R8 后，应先
运行 `build-only` 或内部测试，验证冷启动、登录、主机连接、Agent 会话、通知和原生模块。
如需修复反射或 JNI 兼容性，只添加必要的保留规则，避免整包 `-keep` 或全局关闭优化。
实际 DEX 指标以新 AAB 的 Play Console 结果为准，不承诺固定混淆百分比。
参见 [DEX 指标](https://developer.android.com/google/play/vitals/code-optimization)和
[R8 配置](https://developer.android.com/topic/performance/app-optimization/enable-app-optimization)。

AAB 使用 **上传密钥** 签名。Google Play 安装包由 **Play App Signing 的应用签名密钥**
签名，两种凭据不同。需要站外分发相同应用签名的 APK 时，在 Play Console 的应用版本
下载页面取得 Google 签名的 universal APK；本 workflow 当前只交付 AAB。
见 [Play App Signing](https://support.google.com/googleplay/android-developer/answer/9842756)。

## 重试与验收

- 构建或校验失败：不会上传到 Play，修复后重跑。
- 上传权限或签名错误：检查服务账号的 Play 应用权限，以及 keystore 是否匹配 Play 登记的
  upload certificate。服务账号 JSON 用于 API 身份认证，不能替代签名 keystore。
- 应用仅允许草稿：选择 `internal-draft`，在 Console 完成首次发布。
- 版本号已使用或低于已发布版本：检查迁移基数；不要降低它。较旧 run 在新 run 上传后
  重跑可能仍然版本过低，此时使用新的 Run workflow。
- 超时、取消或上传结果不明：先在 Play Console 按实际 `versionCode` 查明是否已接收，
  再决定是否重跑。重跑会重新构建并分配新版本号，不会自动复用旧 AAB。

Gradle 构建最多 120 分钟，上传最多 30 分钟，整个 job 最多 150 分钟；Play 各次提交串行。
在 Console 发布内部测试后，用测试者账号打开 opt-in 链接并从 Google Play 安装。
检查冷启动、浏览器登录返回、主机连接、消息收发和会话恢复，记录设备、Android 版本及
`versionCode`。Google 签名版本可能无法覆盖既有 GitHub APK，应先处理需要保留的本地数据。

个人账号后续申请正式发布的测试要求见
[Google 官方说明](https://support.google.com/googleplay/android-developer/answer/14151465)。
本流程只覆盖内部测试；Android SDK 构建和 Play 真机验收需在配置凭据后实际运行。
