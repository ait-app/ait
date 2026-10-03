# Ait 发布操作指南

从 0.0.7 起，GitHub Release 构建 `apps/desktop` Electron 桌面、`apps/mobile` 的 Web 导出和
Rust `daemon`。发布边界见 [ADR-053](../decisions/clients/adr-053-paseo-desktop-release.md)，
旧桌面源码的移除见 [当前架构](../architecture/README.md)。

推送标签只自动构建桌面。Android APK 通过独立的手动工作流使用 EAS 构建，
可在桌面 Release 完成后加入同一版本，也可从当前分支创建测试预发布版本。
APK 的签名方式、安装要求与手动测试入口见 [Android APK 发布](android-releases.md)。

`apps/mobile` 的 Google Play Android Internal Testing 发布流程见下方
[Google Play Android Internal Testing](#google-play-android-internal-testing) 一节，
与桌面 GitHub Release 相互独立。`apps/mobile` 的 Apple TestFlight 发布流程见下方
[Apple TestFlight iOS 手动发布](#apple-testflight-ios-手动发布)一节，
同样与桌面 GitHub Release 相互独立。见 [ADR-070](../decisions/clients/adr-070-ios-testflight-release.md)。

## 发布产物

| 平台            | 架构                    | 文件                                                     |
| --------------- | ----------------------- | -------------------------------------------------------- |
| Linux           | x86_64                  | `Ait-linux-x86_64.AppImage`                              |
| Linux           | x86_64                  | `Ait-VERSION-linux-x64.tar.gz`                           |
| macOS           | Apple Silicon arm64     | `Ait-VERSION-macos-arm64.dmg`                            |
| macOS           | Apple Silicon arm64     | `Ait-VERSION-macos-arm64.zip`                            |
| Android（独立发布） | 通用（含 ARM64、ARMv7） | `Ait-VERSION-android.apk`                                |
| 自动更新        | 各平台                  | `latest-linux.yml`、`latest-mac.yml`、生成的 `.blockmap` |
| 校验            | 全部资产                | `SHA256SUMS`                                             |

AppImage 文件名保持稳定，版本体现在 Release 标签和应用内部。Windows、deb/rpm、其他架构
和独立 CLI 不属于本次发布。安装包 `resources/bin/` 中只有 `daemon`；Electron 主程序与
Helper 是必需运行时。GitHub 仍自动提供标签对应的源码归档。

## 准备版本

同步根 `Cargo.toml` 的 `workspace.package.version`、带版本约束的本地 Cargo path 依赖、
Cargo.lock、根 package.json、所有活跃 npm workspace 及其 lockfile。然后运行：

```bash
npm ci
npm run verify:release -- v0.0.15
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
```

更新根 CHANGELOG.md：该文件
会打入应用，供“新功能”页面读取。把版本与发布变更经 PR 合并到 `main` 后，再创建标签。

## 创建 Release

```bash
git switch main
git pull --ff-only
npm run verify:release -- v0.0.15
git tag -a v0.0.15 -m "Ait v0.0.15"
git push origin v0.0.15
```

`.github/workflows/release.yml` 在 Linux x86_64 和 macOS arm64 原生 runner 上构建桌面：

1. 校验标签和全部活跃版本，安装根 npm workspace，验证发布脚本。
2. 用锁定依赖只构建 `daemon` 的 `daemon`。
3. 导出界面、编译 Electron 主进程，验证 daemon 版本并暂存单个可执行文件。
4. 检查打包内容；macOS 签名、公证；隔离启动成品应用并验证真实 daemon 生命周期。
5. 收集桌面安装包和自动更新资产，核对更新摘要。
6. 桌面构建成功后汇总资产并生成 SHA256SUMS，再创建或修复 GitHub Release。

macOS 需要 GitHub Secrets：`MAC_CSC_LINK`、`MAC_CSC_KEY_PASSWORD`、`APPLE_ID`、
`APPLE_BUILD_APP_SECRET`、`APPLE_TEAM_ID`。缺失签名、公证凭据会阻止发布；不会降级为未签名包。
Release Note 由 `.github/release.yml` 根据合并 PR 分组生成。

手动重跑：Actions → Release Ait → Run workflow，输入已存在的标签。工作流不会创建标签。
Android APK 的正式和测试发布由独立的 [Android APK 发布](android-releases.md)流程处理。
已有 Release 保留说明，覆盖同名资产；不要移动已经公开使用的标签。

默认应用源码从输入的发布标签检出；资产收集、校验和相关测试从工作流自身的提交检出到
`.tmp/release-tools`。因此修复发布工具后，可以选择包含修复的工作流分支重跑原始标签：

```bash
gh workflow run release.yml --ref main -f tag=v0.0.7
```

修复尚在 PR 分支时，`--ref` 可以指定该分支。`github.workflow_sha` 固定该次运行使用的
工具提交，应用仍由原始标签构建，不改写标签。普通 Re-run jobs 沿用旧工作流，不能加载
新提交的工具修复。[GitHub 工作流版本说明](https://docs.github.com/en/actions/reference/workflows-and-actions/variables)

明确要求更新既有版本的安装包时，可以额外传入 `source_commit`（必须是完整 40 位 SHA）。
所有平台和发布步骤均从该提交检出，仍须通过各自的版本、签名、公证和成品启动门禁。
这不会移动原标签；`BUILD-INFO.json` 记录源码、工作流提交和运行链接，随校验和一起上传。
必须在 Release Note 中说明重建修复、实际源码提交及关联 PR，提醒同版本用户重新下载安装。
GitHub 自动生成的 Source code 归档仍对应原标签；修复后的源码应链接到 `sourceCommit`。

```bash
gh workflow run release.yml --ref YOUR_PR_BRANCH -f tag=v0.0.7 -f source_commit=FULL_COMMIT_SHA
```

## 本地验证

Linux x86_64：

```bash
npm ci
cargo build --locked --release -p daemon --bin daemon --target x86_64-unknown-linux-gnu
AIT_SERVER_BIN="$PWD/target/x86_64-unknown-linux-gnu/release/daemon" \
  AIT_DESKTOP_SMOKE=1 npm run package:linux
```

Linux 需安装 `xvfb`、FUSE 和 Electron 的系统库；具体包名见 workflow。

macOS arm64（正式签名、公证）：

```bash
npm ci
# 配置 CSC_NAME 或 CSC_LINK，以及 Apple 公证凭据后：
AIT_DESKTOP_SMOKE=1 npm run package:mac
```

`package:mac` 生成 DMG 与 ZIP；`package:linux` 生成 AppImage 与 tar.gz。省略 `AIT_SERVER_BIN`
会从当前源码构建 release daemon；提供该变量时仍检查二进制版本。只允许原生目标平台、架构。

无签名凭据时，本机开发验证使用 `AIT_DESKTOP_SMOKE=1 npm run build:dmg`。它生成
`Ait-VERSION-local-arm64.dmg`，不属于正式发布文件，不能通过正式资产收集门禁。

输出位于 `apps/desktop/release/`，暂存输入位于 `apps/desktop/release-resources/daemon/`，均不提交。
下载后用 Linux `sha256sum -c SHA256SUMS` 或 macOS `shasum -a 256 -c SHA256SUMS` 校验。

## 兼容性与失败恢复

新应用沿用 Ait 正式应用 ID `dev.ait.desktop`。独立 daemon 默认使用
`~/.ait-server-desktop`；旧桌面的 SQLite 数据保留，但本次不自动迁移到新 daemon。

- 版本或包内容验证失败：修复 manifest、锁文件或暂存输入，重新运行门禁。
- 任一平台构建、签名或启动失败：Release job 不会运行。
- 发布阶段失败：在同一不可变标签上手动重跑，补齐资产。
- 已公开的错误版本：发布新的补丁版本，不移动标签。

## Google Play Android Internal Testing

`apps/mobile` 的 Android 构建通过 EAS Build/Submit 发布到 Google Play 的 **Internal testing**
渠道，用于真机验证，与上文桌面 GitHub Release 完全独立，不经过 `.github/workflows/release.yml`。
本节只覆盖 Internal testing；正式商店发布不在本节范围内，流程和门禁仍需另行确认。

### 前提条件

在第一次运行任何命令前，需要先在 Google Play Console 确认或完成：

- 一个可用的 Google Play 开发者账号，并已创建对应 app，package name 为 `dev.ait.mobile`
  （见 `apps/mobile/app.config.js` 的 `production` variant；`development` variant 用的是
  `dev.ait.mobile.debug`，不用于 Play 发布）。仓库里不记录这个 app 是否已经在 Play Console
  创建，执行前需要自己去 Play Console 确认。
- app 的签名方式。首次创建 app 时 Play Console 会要求选择 **Choose signing key**，官方推荐
  **Google-generated key**（即 Play App Signing），这样即使本地 upload key 丢失也能继续发布。
- 一个 Google Service Account JSON key，并已经上传到 EAS 的项目 credentials 里。这是
  `eas submit --platform android` 鉴权用的凭据，创建步骤见
  [Creating a Google Service Account key](https://expo.fyi/creating-google-service-account)，
  上传步骤见 [Submit to the Google Play Store with EAS Submit](https://docs.expo.dev/submit/android/)
  的 "Create a Google Service Account key" 一节。JSON key 文件本身绝不能提交到仓库；
  上传后凭据保存在 Expo 服务器端，本地不需要长期保留这份文件。
- Internal testing 渠道的测试者名单（Play Console → Testing → Internal testing → Testers），
  参考 [Set up an open, closed, or internal test](https://support.google.com/googleplay/android-developer/answer/9845334)。
  没有测试者名单时构建仍能上传，但测试者收不到安装链接。

仓库侧不持有、也不应该持有上述任何账号凭据、service account key 或测试者名单；它们都在
Google Play Console 和 EAS 的账号系统里维护。

### app 身份与版本号

- Android package：`dev.ait.mobile`（`apps/mobile/app.config.js` 的 `variants.production.packageId`）。
- 版本号来自根 `package.json`/`apps/mobile/package.json` 的 `version` 字段，由
  `apps/mobile/native-release-version.js` 的 `getNativeReleaseVersion()` 派生：`appVersion` 原样用作
  Android `versionName`，`androidVersionCode` 由 `major*1_000_000 + minor*1_000 + patch` 计算。
  例如当前 `package.json` 的 `0.0.11` 会派生出 `versionCode = 11`。构建前确认两处 `package.json`
  版本一致，发布准备流程与桌面发布共用同一份[准备版本](#准备版本)步骤。

### EAS 构建与提交 profile

`apps/mobile/eas.json` 里和 Internal testing 相关的 profile 只有这两个：

- `build.ait`：`extends: "production"`，`distribution: "store"`，用于生成可提交 Play Store 的
  `.aab`；同时带着 iOS 的 `EXPO_OWNER`/`EXPO_SLUG`/`EAS_PROJECT_ID`/`APPLE_TEAM_ID` 等公开标识，
  不含任何密钥。
- `submit.ait.android`：`{ "track": "internal", "releaseStatus": "completed" }`，提交后直接出现
  在 Internal testing 渠道，不会碰 production 轨道。

### 安全的构建 / 提交命令

全部命令从 `apps/mobile` 目录运行：

```bash
cd apps/mobile
eas build --platform android --profile ait
```

构建完成后用返回的 build id 提交（或用 `--latest` 提交最近一次成功构建）：

```bash
eas submit --platform android --profile ait --id <build-id>
# 或
eas submit --platform android --profile ait --latest
```

`--profile ait` 是关键参数：它锁定使用 `submit.ait.android`，保证目标轨道是 `internal`。
两条命令都可以加 `--non-interactive`，配合已上传的 Google Service Account key 在 CI 环境执行。

### 产物检查

- `eas build --platform android --profile ait` 结束后会打印 build id、build 详情页 URL 和产物
  下载链接；执行前先用 `eas build:list --platform android --limit 5` 或打开详情页确认这是预期的
  commit/版本，再继续提交。
- 可选：`eas build:view <build-id>` 查看该次构建使用的 profile、`versionCode`/`versionName`
  和 Git commit，核对与上面"app 身份与版本号"一节推导出的数值一致。
- `eas submit` 提交成功后，去 Play Console 的 Internal testing 页面确认新版本已经出现在
  release 列表里，状态变为可供测试者安装。

### 首次上传 / 手动上传规则

如果这是该 app 在 Play Console 的第一次发布，按官方说明有两条路径，二选一：

1. 先在 Play Console 完成 [Manually submit an Android app to the Google Play Store](https://docs.expo.dev/submit/android-manual) 的步骤，手动上传第一个 `.aab`、创建第一个 release；之后的版本再用上面的
   `eas submit --profile ait` 命令提交。
2. 直接用 `eas submit --platform android --profile ait`。官方文档说明：只要 app 已经在 Play
   Console 创建（即使商店资料、隐私政策等还没填完），且 EAS 已经配置好 Google Service Account
   key，`eas submit` 就可以创建该 app 在 Play Console 的第一个 release，并默认落在 internal
   testing 轨道。

两条路径都要求 app 已经在 Play Console 创建，这一步无法跳过，也不能靠命令行自动完成。
选哪条路径取决于个人偏好，仓库里没有记录哪条路径已经执行过，发布前需要自己去 Play Console
确认当前状态。

### 真机冒烟测试（连接 Rust daemon）

当前仓库 README 和 [Apple 构建说明](apple-builds.md) 都明确没有做过真机网络访问验证：
独立 Rust daemon 默认只监听 loopback，真机访问电脑上的 daemon 需要额外的网络入口或隧道，这一步
在本仓库里还没有被验证过，执行 Internal testing 发布时需要自己完成，不能假定已经打通。

建议的手动验证步骤：

1. 在一台可被测试手机访问的机器上启动 Rust daemon，并显式监听非 loopback 地址，例如：
   ```bash
   cargo run -p daemon --bin daemon -- --data-dir /path/to/data --listen 0.0.0.0:7316
   ```
   需要自行解决真机到这台机器的网络可达性（同一局域网、内网隧道或其他方式），仓库当前不提供
   现成方案。
2. 在 Play Console 的 Internal testing 页面把测试者邮箱加入 Testers 名单，并通过 opt-in 链接
   在真机上安装刚提交的版本。
3. 打开 app，按照连接表单填写 daemon 的 Host、端口 `7316` 和当前 daemon 的访问令牌，确认能
   正常建立连接、收发消息，覆盖一次完整的鉴权 + RPC 往返。
4. 把实际验证到的机型、Android 版本和结果记录下来；本节本身不包含任何已完成的验证结果。

### 明确禁止事项

- **不要**使用 `submit.production`（对应 `track: "production"`）来做 Internal testing 提交；
  它会把构建推到正式商店轨道，不是内部测试轨道。
- **不要**运行或触发 `.eas/workflows/release-mobile.yml`、`.eas/workflows/release-ios-beta.yml`
  或 `.eas/workflows/resubmit-ios-review.yml`。这些 workflow 现在只保留手动触发，但仍使用
  `profile: production` 或 fastlane 旧应用路径；`release-mobile.yml` 的
  `submit_ios_for_review` 还会调用 fastlane 提交 App Store 审核。这些是桌面之外的遗留正式发布
  流水线，和本节的内部测试流程无关，误触会导致未经验证的构建被提交审核。
- `apps/mobile/scripts/eas-submit-tracks.test.cjs` 和
  `apps/mobile/scripts/ios-testflight-workflow.test.cjs` 是保护上述边界的回归测试，确认
  `submit.ait.android` 始终是 `{ track: "internal", releaseStatus: "completed" }`、
  `submit.production.android` 始终是 `{ track: "production", releaseStatus: "completed" }`，
  并且遗留 EAS workflow 没有 `push` 触发器。改动 `eas.json` 或这些 workflow 前，先用
  `node --test apps/mobile/scripts/eas-submit-tracks.test.cjs` 确认没有破坏这个边界；该测试文件
  与 `eas.json` 本身属于移动发布边界的一部分。

## Apple TestFlight iOS 手动发布

`apps/mobile` 的 iOS 构建通过 GitHub Actions 手动工作流
`.github/workflows/release-ios-testflight.yml` 发布到 App Store Connect 的 TestFlight，
用于内部测试组安装，与上文桌面 GitHub Release 和 Google Play Internal Testing 完全独立，
不经过 `.github/workflows/release.yml` 也不经过 EAS 自带的 `.eas/workflows/*`。
完整设计见 [ADR-070](../decisions/clients/adr-070-ios-testflight-release.md)。

### 触发方式与固定顺序

该工作流只能手动触发（`workflow_dispatch`），没有 `push` 触发器，不会在打标签或合并到
`main` 时自动运行；`if: github.ref == 'refs/heads/main'` 还把运行限制在 `main` 分支。
发布顺序固定为三步，不能跳过或调换：

1. 按上文[创建 Release](#创建-release)完成桌面发布，确认存在一个稳定的 `vX.Y.Z` 标签
   （不接受 `-beta`、`-rc` 等预发布标签），并且该标签已经有对应的 GitHub Release。
2. 确认该标签的桌面 Release 已经发布完成（`release.yml` 产出的 Linux/macOS 资产齐全）。
3. 手动运行 `Release iOS TestFlight` 工作流，输入同一个已存在的稳定标签：

   ```bash
    gh workflow run release-ios-testflight.yml --ref main -f tag=v0.0.13
   ```

   `tag` 必须指向已经打好、已经发布且包含 `build.ait` 和 `submit.ait.ios` 的标签；
   `v0.0.13` 仅示意下一个符合条件的版本，发布前须实际准备并创建该标签。已发布的
   `v0.0.11` 源码不含这些 profile，不能用它启动此工作流。工作流不创建、也不移动标签。

### 版本与标签门禁

工作流用 `scripts/verify-release-version.mjs` 校验输入标签是否为稳定 `vX.Y.Z` 形式，并且
与根 `Cargo.toml`、`package.json`、`package-lock.json` 的版本一致。这与上文
[准备版本](#准备版本)用的是同一个脚本，没有另外维护一套 iOS 专用的版本校验逻辑。
通过版本校验后，工作流再用 `gh release view` 确认该标签存在非 draft、非 prerelease 的
正式 GitHub Release；没有找到符合条件的 Release 时工作流直接失败，不会继续构建。

### `ios-testflight` 环境与审核人

发布 job 绑定 GitHub Environment `ios-testflight`。在仓库 Settings → Environments 为该
environment 配置 required reviewers 后，手动触发工作流还需要配置的审核人批准才会继续执行，
构成触发者之外的第二道人工确认。仓库侧不记录当前已配置的审核人名单，增删审核人需要直接在
GitHub 仓库设置里维护。

### 唯一 Secret：`EXPO_TOKEN`

该工作流只声明一个 GitHub Secret：`EXPO_TOKEN`，供 `eas` CLI 以 Access Token 身份非交互
调用 EAS Build 和 EAS Submit。仓库和工作流都不保存、不传递任何 Apple 签名材料，没有
`APPLE_ID`、`APPLE_TEAM_ID` 对应的密码或 App Store Connect API key 作为 Secret。iOS 签名
证书、provisioning profile 和 App Store Connect (ASC) 凭据完全由 EAS 托管，保存在 Expo
项目的 credentials 里；构建步骤显式传入 `--freeze-credentials`，锁定复用 EAS 已保存的签名
配置，不触发新的凭据生成，runner 本地也不落地任何证书或 API key 文件。工作流从标签中
`build.ait.env` 读取公开的 Expo owner、slug、EAS project ID、bundle ID 和 Apple team ID，
传给后续 `build:list`、`build:view`、构建及提交命令；这些公开标识不是额外的 Secret。
首次运行前须在 EAS 为该应用配置 **提交用** App Store Connect API key；已保存的构建签名
证书不能代替它。此前 `0.0.11` 的成功提交使用的是本机 `.p8`，不证明 EAS 已存该 key。

### `ait` profile

EAS 构建与提交统一使用 `apps/mobile/eas.json` 里的 `ait` profile：

- `build.ait`：`extends: "production"`，携带 `IOS_BUNDLE_IDENTIFIER=com.necokeine.ait`、
  `APPLE_TEAM_ID` 等公开标识，不含任何密钥。
- `submit.ait.ios`：固定 `ascAppId`、`appleTeamId`、`bundleIdentifier` 三项，指向 Ait 在
  App Store Connect 的 app，`appleTeamId` 与 `build.ait.env.APPLE_TEAM_ID` 保持一致。

工作流的构建与提交步骤都显式传入 `--profile ait`，不使用 `eas.json` 里 Paseo 遗留的
`production`、`simulator`、`preview` 等其它 profile。

### 重复构建拦截与 `build_id` 重试

没有显式提供 `build_id` 输入时，工作流先用
`eas build:list --platform ios --build-profile ait --app-identifier com.necokeine.ait
--app-build-version <build_number>` 查询是否已存在同一 iOS build number 的构建。只要存在
状态为 `finished`、`in-queue`、`in-progress`、`new` 或 `pending-cancel` 的匹配构建，就视为
不安全的重复构建：工作流立即失败，报错信息带上已存在构建的 build ID，不会再发起新的 EAS
构建。只有同一 build number 下的匹配构建全部是 `errored` 或 `canceled`，才允许继续发起新构建。

显式传入 `build_id` 输入时，工作流跳过新建构建，改为校验这个 build：必须匹配 `ait`
profile、对应的 app 版本和 iOS build number，并且状态为 `finished`；校验通过才直接提交
该 build 到 TestFlight，校验不通过（profile/版本不匹配或尚未构建完成）则拒绝执行，不会
静默退回去新建一次构建。这条路径用于构建已经成功、只是提交步骤失败时的重试，不需要重新
跑一遍完整构建：

```bash
gh workflow run release-ios-testflight.yml --ref main -f tag=v0.0.13 -f build_id=<existing-build-id>
```

### 不提交 App Store 审核

工作流只把构建提交到 TestFlight（`eas submit --platform ios --profile ait --id <build-id>
--non-interactive --wait`），不调用任何 App Store 正式审核提交命令。运行结束会把结果写入
`GITHUB_STEP_SUMMARY`，其中明确写明“App Store review: not submitted (manual in App Store
Connect)”。是否提交正式审核、何时提交，需要人工登录 App Store Connect 另行操作，不属于本
工作流范围，也不会被任何自动化触发。

### 明确禁止事项

- **不要**运行或触发 `apps/mobile/.eas/workflows/release-mobile.yml`、
  `apps/mobile/.eas/workflows/release-ios-beta.yml` 或
  `apps/mobile/.eas/workflows/resubmit-ios-review.yml`。三者都是 Paseo 遗留的正式发布流水线：
  `release-mobile.yml` 在 `submit_ios_for_review` job 里调用 `bundle exec fastlane ios
submit_review` 直接提交 App Store 审核；`release-ios-beta.yml` 用 `profile: production`
  构建，并通过 `type: testflight` 的 `submit_beta_review: true` 直接提交外部测试组审核；
  `resubmit-ios-review.yml` 同样调用 fastlane 重新提交审核。三者都不经过本节描述的
  `ios-testflight` environment 审核人门禁、`EXPO_TOKEN`-only Secret 边界，也不使用 `ait`
  profile，误触会导致未经本节流程校验的构建被提交审核。
- **不要**在本工作流或相关脚本里安装、调用 fastlane，也不要引入任何绕开 `ait` profile
  直接调用 App Store 审核提交的命令；本节描述的工作流设计上不依赖 fastlane。
- **不要**在 `ios-testflight` environment 或该工作流的 run 里添加除 `EXPO_TOKEN` 以外的
  Apple 签名、ASC API key 或 Apple ID 相关 Secret；签名与 ASC 凭据边界完全交给 EAS 托管，
  仓库侧新增这类 Secret 即违反本节描述的设计边界。
