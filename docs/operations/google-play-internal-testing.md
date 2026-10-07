# Google Play 内部测试与 CI 发布

[Release Android to Google Play](../../.github/workflows/release-android-play.yml) 手动构建签名
AAB，并按选择提交到 Internal testing。包名固定为 `dev.ait.mobile`，使用现有
`sd542927172s-team/ait` EAS 项目。发布设计见
[ADR-097](../decisions/clients/adr-097-google-play-internal-release.md)。

## 首次配置

### 1. 创建应用和测试者名单

在 Play Console 创建 Ait 应用，然后打开「测试和发布 → 测试 → 内部测试 → 测试者」。
创建 `ait-internal` 邮箱名单，加入测试手机使用的 Google 账号邮箱，勾选该名单并保存。
首轮可以只加入维护者自己；Internal testing 最多支持 100 名测试者。

应用的包名由首个上传的 AAB 确定。首次上传前确认它是 `dev.ait.mobile`，后续不能更换。
内部测试可以在完成全部商店资料前开始；参见
[Google 内部测试说明](https://support.google.com/googleplay/android-developer/answer/9845334)。

### 2. 配置 Google Cloud 发布身份

在 [Google Cloud Console](https://console.cloud.google.com/) 选择现有项目，或创建
`ait-play-store` 项目。在「API 和服务 → 库」启用 **Google Play Android Developer API**。
然后在「IAM 和管理 → 服务账号」创建 `ait-play-publisher`。仅为 Play 发布使用时，
创建向导中的 Google Cloud 项目角色和其他用户访问项可以留空；实际发布权限在 Play
Console 中授予。记录生成的服务账号邮箱。

在该服务账号「密钥 → 添加密钥 → 创建新密钥」下载 JSON 密钥。不要将其放入仓库，
也不要粘贴到聊天、日志或 issue。Google 已不再要求将开发者账号与 Cloud 项目关联；
具体步骤见 [Google Play Developer API 入门](https://developers.google.com/android-publisher/getting_started)。

### 3. 授予 Ait 的内部测试权限

回到 Play Console 的开发者账号层级，打开「用户和权限 → 邀请新用户」，填写服务账号邮箱。
在应用权限中仅添加 Ait，并授予：

- 查看应用信息（只读）。
- 将应用发布到测试轨道（Release apps to testing tracks）。

本 CI 不维护测试者名单，无需授予「管理测试轨道和修改测试者名单」，也无需账号管理员、
财务或正式版发布权限。权限名称和作用见
[Play Console 权限说明](https://support.google.com/googleplay/android-developer/answer/10019561?hl=en)。

### 4. 将提交密钥交给 EAS

打开 [Ait EAS 项目](https://expo.dev/accounts/sd542927172s-team/projects/ait)，进入
「Credentials → Android → dev.ait.mobile → Service Credentials」，选择
「Add / Change Google Service Account Key」并上传刚下载的 JSON。已有服务账号密钥时，
先确认其 Play 权限，再决定是否替换。上传步骤见
[Expo Android 提交说明](https://docs.expo.dev/submit/android/)。

JSON 由 EAS 保存；GitHub Actions 只需要现有仓库 Secret `EXPO_TOKEN`，对应的 Expo 账号
须有该项目的构建和提交权限。不要另建存放 JSON 的 GitHub Secret，也不要提交密钥文件。

Android keystore 是另一种凭据：它签署上传包，服务账号密钥用于调用 Play API。
沿用既有 `dev.ait.mobile` EAS keystore。CI 使用 `--freeze-credentials`，不会创建或更换
签名密钥。如果新 profile 尚未关联签名凭据，可在已登录 Expo 的终端配置一次：

```bash
cd apps/mobile
npx --no-install eas credentials:configure-build --platform android --profile production-play
```

选择复用既有 production 应用的 keystore。不要为同一应用另换上传密钥。

## 首个版本

### 5. 先构建并查看 AAB

工作流合并到默认分支后，打开 GitHub Actions → **Release Android to Google Play** →
**Run workflow**。选源码分支，保留默认 `action=build-only` 和空 `tag`。
这会构建所选分支触发时的 commit。也可指定稳定标签 `vX.Y.Z`，使用该标签源码；
Play 内测不要求先发布桌面 GitHub Release。

构建成功后，run summary 给出 EAS build ID、源码 commit、版本和实际 `versionCode`。
下载 artifact `ait-android-play-aab`，包含 `.aab`、`BUILD-INFO.json` 和 `SHA256SUMS`。
CI 检查构建来源、包名、版本、非 debug 属性、bundle 结构和上传签名，并保留已签名字节。

```bash
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=build-only
```

### 6. 上传内部测试草稿

服务账号配置好后，手动运行同一工作流，选择 `action=internal-draft`。它构建并上传新的
AAB，将 release 保留为草稿，方便在 Play Console 检查首轮设置。
Expo 当前支持直接通过 EAS Submit 上传首个版本，不要求先手动上传，见
[首次提交说明](https://docs.expo.dev/submit/android/#first-time-submission)。

若步骤 5 的构建已经通过验证，也可以直接提交这个 build ID，避免重复构建：

```bash
cd apps/mobile
npx --no-install eas submit --platform android --profile play-internal-draft --id BUILD_ID --wait
```

这里的 `BUILD_ID` 必须来自上述通过验证的 Play AAB run；不要用 `--latest` 猜测构建。
若希望手动完成首轮，也可以在 Play Console「内部测试 → 创建新版本」上传步骤 5 的
AAB；两种方式二选一，同一个 `versionCode` 不应重复上传。

首次上传时，按 Play Console 提示检查 Play App Signing。通常可使用 Google 生成的
应用签名密钥，EAS keystore 继续作为上传密钥。两者用途见
[Play App Signing 说明](https://support.google.com/googleplay/android-developer/answer/9842756)。
Google 签名的 Play 安装包可能无法覆盖已有的 GitHub APK，因为应用签名证书不同。
可使用没有安装 APK 的测试设备；若选择卸载旧 APK，先处理需要保留的本地数据。

### 7. 发布并安装

在 Play Console 检查内部测试草稿的包名、版本、签名和错误提示，填写简短的版本说明，
保存并发布到内部测试。回到「测试者」，复制参与测试的 opt-in 链接。
使用名单中的 Google 账号在 Android 手机上打开链接，加入测试，再从 Google Play 安装。
新应用或链接可能需要等待处理后才可访问，以 Console 状态为准。

确认首次安装成功后，后续 CI 可以直接选择 `action=internal`：

```bash
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=internal
gh workflow run release-android-play.yml --repo ait-app/ait --ref main -f action=internal -f tag=vX.Y.Z
```

`internal` 使用 `releaseStatus=completed`，将本次通过验证的 build ID 提交给内部测试者。
三个 action 均为手动触发；推送标签和合并分支不会自动提交，工作流没有 production 选项。

### 8. 真机验收

从 Play 安装后检查冷启动、Authing 浏览器登录与返回应用、主机列表、连接 daemon、
收发一轮消息和会话恢复。使用可访问的主机或现有中继配置，并验证需要的原生权限。
记录机型、Android 版本、`versionCode` 和结果，不能仅用构建或提交成功代替真机验收。

## 版本号和重试

`versionName` 仍取自仓库版本。Play CI 在临时 runner 配置中启用 EAS remote
`appVersionSource`，并使用 `production-play.autoIncrement=true`，使相同源码重复构建也
获得新的 Android `versionCode`。仓库配置保留 local 版本来源，APK 与 TestFlight 的
发布流程继续使用原有规则。远端计数器属于 EAS 项目和 Android 应用，会跨 Play 构建复用。
参考 [Expo 构建版本说明](https://docs.expo.dev/build-reference/app-versions/)。

- 构建或本地验证失败：本次不会提交到 Play；修复后重跑会创建新构建。
- 上传失败：先看 EAS Submit 日志。确认权限、API 或应用设置后，可按步骤 6 提交同一个
  尚未被 Play 接收的 build ID，无需重新构建。
- 草稿应用提示只能上传 draft：选择 `internal-draft`，在 Console 完成首轮发布。
- `versionCode` 已使用：核对 Play 中历史最大值和 EAS 远端计数器。若此前通过别的流程
  上传过更高版本，在使用 remote 配置的临时 checkout 中运行
  `eas build:version:set --platform android --profile production-play` 设置已有最大值，
  再构建；不要降低计数器。
- GitHub 超时或取消：查看 EAS 控制台的构建/提交任务；它们可能仍在运行，先核对状态。

## 个人账号后续正式发布

新个人开发者账号可以先开展 Internal testing。申请 production 权限前，需要完成应用
设置并进行至少 12 名测试者连续参与 14 天的 **Closed testing**，之后向 Google 申请。
内部测试不计入这项封闭测试要求，见
[个人账号测试要求](https://support.google.com/googleplay/android-developer/answer/14151465)。
正式商店发布与封闭测试配置留待完成首轮内部测试后处理。

以上外部流程于 2026-10-07 对照 Google 与 Expo 官方文档核实。
