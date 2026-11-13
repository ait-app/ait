# Ait 发布操作指南

发布入口按平台划分；版本准备统一从[准备版本](#准备版本)开始。

| 产物                                            | 入口                                            | 操作说明                                            |
| ----------------------------------------------- | ----------------------------------------------- | --------------------------------------------------- |
| Linux/macOS/Windows 桌面、独立 daemon、Web 镜像 | 标签或手动 `release.yml`                        | [创建 Release](#创建-release)                       |
| Linux/macOS Dev build 与独立 daemon             | 手动 `dev-build.yml`                            | [Dev build](#dev-build)                             |
| Android Google Play 内部测试                    | 稳定标签或手动 `release.yml`                    | [Google Play 指南](google-play-internal-testing.md) |
| iOS 内部 TestFlight                             | 稳定标签或手动 `release.yml`                    | [内部测试](#apple-testflight-ios-内部测试)          |
| iOS 外部 TestFlight / App Store 构建            | Xcode Cloud `Release iOS to App Store` 手动构建 | [App Store 构建](#app-store-ios-手动发布)           |

Android 和 iOS 内部测试都随 `release.yml` 发布，失败不阻塞桌面 Release；外部 TestFlight 与 App Store 仍手动。
Android 使用 Gradle，iOS 使用 Xcode Cloud，两者都不使用 EAS CI。

## macOS 自动更新来源

新 macOS 安装包内嵌的更新源为 `https://aitapp.cn/api/v1/updates/macos/`，
稳定版和 beta 分别读取 `latest-mac.yml`、`beta-mac.yml`。AIT Server 提供清单及按地区
跳转的版本化文件入口；国内及未知地区下载 COS，其他已知地区下载 GitHub。
Linux 和 Windows 使用 GitHub 更新源；Windows 对应 `latest.yml` / `beta.yml`。
见 [ADR-165](../decisions/clients/adr-165-macos-ait-server-updates.md)。

GitHub Release 仍需上传 builder 生成的 DMG、ZIP、对应 blockmap 和更新 YAML；
ait-realease 发布编排核对 `SHA256SUMS`，将相同文件上传至 COS 的 `v{version}/`，
验证可读后最后更新 `latest-macos.json`。清单中保留完整 UpdateInfo 元数据，只在发布时
把 `files[].url` 及兼容字段 `path` 转为 `files/v{version}/{filename}`。
旧版本 ZIP 与 blockmap 也需保留，以支持差分更新。beta 清单需写入 `channels.beta`；
未发布通道返回 404，客户端显示无可用更新，503 或网络故障仍显示检查错误。

客户端设置 `useMultipleRangeRequest: false`，适配 COS 的单段 Range 支持。
已安装的旧版本仍从 GitHub 获取首次升级，安装包含新配置的版本后才使用 AIT Server。
本地开发不检查更新，正式交付时需验证公开清单、文件跳转和真实签名安装包升级。

## Dev build

[dev-build.yml](../../.github/workflows/dev-build.yml) 工作流只能在 Actions 中对 `main` 手动触发，
不会定时或随推送运行。每次运行发布 `main` 当前提交的 Linux x86_64 和 Apple Silicon 安装包，替换固定的
[dev 预发布](https://github.com/ait-app/ait/releases/tag/dev)。CI 本身只负责检查，不打包也不发布。

- 准入：等待该提交在 `main` 上的 CI 结束（最长一小时），只有 CI 成功才构建；CI 失败、被取消或
  超时时保留上一版，并在运行中给出警告。对已发布的提交再次触发会重新构建并替换。
- 构建：两个平台全部成功后才发布。同一时间只运行一个 dev build，后触发的运行排队等待，
  不会中断正在进行的上传。每次运行都发布触发时 `main` 的提交，因此 dev build 不会回退。
- 发布：先把 `dev` 标签移到本次提交，再替换 Release；标签移动失败时已发布的 dev build
  保持不变。如果 ruleset 保护该标签（422 “Cannot update this protected ref”），标签留在原提交，
  Release 仍发布本次构建并给出警告；要让标签跟随构建，需要把 GitHub Actions 加入该 ruleset
  的 bypass 列表。
- 无需创建版本标签。每次运行只清理自身的临时产物。

dev build 的公开构建标识统一为 `<8位提交hash>-<构建UTC日期>`，例如 `38fb6054-2026-10-08`。
Release 标题为 `Ait Dev 38fb6054-2026-10-08`，安装包名例如
`Ait-38fb6054-2026-10-08-macos-arm64.dmg`、`Ait-38fb6054-2026-10-08-linux-x86_64.AppImage`。
ZIP、tar.gz、blockmap、CI 产物和 `BUILD-INFO.json` 使用同一标识；同次运行的两平台共用准备步骤生成的标识；日期来自构建时间，重跑准备步骤可能改变日期。
更新元数据沿用更新器识别的 `latest` / `beta` 入口文件名，内部引用对应的新安装包名。
应用与 daemon 显示 `YYYY-MM-DD-<8位hash>`；更新器使用日期型 SemVer，在构建信息中记作 `packagedVersion`。
命名规则以 [`dev-build.mjs`](../../scripts/dev-build.mjs) 和 [`source-version.cjs`](../../scripts/source-version.cjs) 为准。
固定下载地址仍使用 `dev` 标签；正式版本继续使用版本命名。

Release 说明记录源码提交；`BUILD-INFO.json` 记录源码提交、
运行序号和运行链接，可用于确认安装包是否包含指定修复。
macOS dev build 使用 ad-hoc 签名，未公证。

dev build 同样附带独立 daemon 归档，例如 `Ait-38fb6054-2026-10-08-daemon-linux-x64.tar.gz`，
与桌面包共用同一次构建的 daemon。macOS 归档只有 ad-hoc 签名、未公证；用浏览器下载后若被拦截，
在解压目录执行 `xattr -d com.apple.quarantine aitd`。见 [ADR-163](../decisions/clients/adr-163-nightly-standalone-daemon.md)。

## PR CI

创建或更新 PR 会运行适用的 Rust、UI、Web 客户端检查，以及全仓库的文档与格式检查。
推送到 `main` 也只运行同样的检查。CI 不打包桌面端，也不发布任何产物；
Windows 只在 [Release Ait](../../.github/workflows/release.yml) 中构建。

dev build 安装包只在手动触发 [Dev build](#dev-build) 工作流时构建；合并前如需验证安装包，
可按[桌面端构建说明](../../apps/desktop/README.md)在本地打包。

Google Play 和 iOS TestFlight 不随 PR、main CI 或 Dev build 构建；稳定版 Release 会自动发布两者，iOS 也可在 Cloud 手动启动。
独立 APK 构建入口已移除。main/PR CI 不调用 EAS 构建或提交。

## 发布产物

| 平台              | 架构                | 文件                                                                   |
| ----------------- | ------------------- | ---------------------------------------------------------------------- |
| Linux             | x86_64              | `Ait-linux-x86_64.AppImage`                                            |
| Linux             | x86_64              | `Ait-VERSION-linux-x64.tar.gz`                                         |
| Linux（deb）      | x86_64              | `Ait-VERSION-linux-amd64.deb`                                          |
| Linux（rpm）      | x86_64              | `Ait-VERSION-linux-x86_64.rpm`                                         |
| macOS             | Apple Silicon arm64 | `Ait-VERSION-macos-arm64.dmg`                                          |
| macOS             | Apple Silicon arm64 | `Ait-VERSION-macos-arm64.zip`                                          |
| Windows           | x64                 | `Ait-VERSION-windows-x64-setup.exe`                                    |
| Windows           | x64                 | `Ait-VERSION-windows-x64.zip`                                          |
| Android           | 通用                | `Ait-VERSION-android.apk`（Google 签名，稳定版，稍后附加）             |
| Linux 独立 daemon | x86_64              | `Ait-VERSION-daemon-linux-x64.tar.gz`                                  |
| macOS 独立 daemon | Apple Silicon arm64 | `Ait-VERSION-daemon-macos-arm64.tar.gz`                                |
| 自动更新          | 各平台              | `latest-linux.yml`、`latest-mac.yml`、`latest.yml`、生成的 `.blockmap` |
| 构建来源          | Windows             | `BUILD-INFO-windows.json`                                              |
| 校验              | 全部资产            | `SHA256SUMS`                                                           |

AppImage 文件名保持稳定，版本体现在 Release 标签和应用内部。deb 与 rpm 的包名为
`ait-desktop`，安装到 `/opt/Ait`，并列入 `latest-linux.yml` 供原生更新器使用。Linux/macOS
安装包 `resources/bin/` 中只有 `aitd`；Windows 不附带 daemon，仅连接远程主机。
Windows Release 构建运行在 `windows-2025`，不安装 Rust 工具链、不构建独立 daemon。
未配置 Windows 代码签名证书时产物未签名。Electron 主程序与 Helper 是必需运行时。
GitHub 仍自动提供标签对应的源码归档。

Linux 的 deb、rpm 和 Arch 包声明 `bubblewrap >= 0.8.0` 为运行时依赖，安装时由包管理器处理。
AppImage、桌面 tar.gz 和独立 daemon 归档不捆绑 `bwrap`，使用沙箱化 Worktree 前需自行安装同版本要求的
bubblewrap，并确保主机允许非特权 user namespace；见 [Linux 沙箱说明](../decisions/workspace/adr-223-linux-worktree-sandbox.md)。

向已发布版本追加 Windows 产物时，保持原标签和 Linux/macOS 文件不变；上传 EXE、ZIP、
更新 YAML、blockmap 与 `BUILD-INFO-windows.json`，在原 `SHA256SUMS` 中追加这些文件的校验和。
Windows 构建来源单独记录，原 `BUILD-INFO.json` 继续描述既有平台。Release CI 使用
`--windows` 强制要求完整 Windows 产物集；历史版本校验仍允许不含 Windows 的旧版本。

独立 daemon 归档供没有桌面环境的主机运行 [无人值守 daemon](headless-daemon.md)，与桌面包使用
同一次构建的 Rust `daemon`。归档内为可执行文件 `aitd` 和 `LICENSE`，与桌面包内的文件同名；改名是为了
放入 `PATH` 时不与系统同名程序冲突；`--help` 和 `--version` 中的程序名也统一为 `aitd`。见 [ADR-212](../decisions/clients/adr-212-packaged-daemon-name-aitd.md)。macOS 版本单独用 Developer ID
签名（hardened runtime）并公证；裸可执行文件无法 staple 公证票据，Gatekeeper 首次运行时在线核验。
Linux daemon 在 `ubuntu-latest` runner 上通过 [`build-linux-daemon.sh`](../../scripts/build-linux-daemon.sh) 于 `manylinux_2_28` 容器内编译，桌面包与独立归档共用；构建后检查不超过 `GLIBC_2.28` 与 `GLIBCXX_3.4.25`，支持 Ubuntu 20.04、Debian 10、RHEL 8 及更新版本。
正式版、beta 和 dev build 都包含这两个归档；dev build 见上文 [Dev build](#dev-build)。

本地打包独立 daemon 归档复用既有发布实现：仓库根运行
`npm run package:daemon -- linux|mac VERSION BINARY DEST [--dev LABEL]`，等价于
`node scripts/release-assets.mjs daemon …`（校验二进制 `--version`、打包 `aitd` 与 `LICENSE`、
dev 标签命名），不引入新的打包路径；CI 仍直接调用同一脚本。

## Arch Linux 本地源码安装

在仓库根目录运行（需要 Arch 的 `base-devel`）：

```bash
makepkg -si
```

根目录的 [`PKGBUILD`](../../PKGBUILD) 构建当前工作区，包含未提交修改和未被 Git
忽略的新文件。它先复制源码再编译 Rust daemon、Web 界面和 Electron 主进程；Electron
运行时和 npm/Cargo 依赖仍从上游下载。不会在原仓库生成 `src/`、`pkg/`、`node_modules/`、
`target/` 或安装包，也不会改写原仓库的版本文件。

源码副本、依赖缓存、临时文件、日志和 `.pkg.tar.zst` 默认位于
`${XDG_CACHE_HOME:-$HOME/.cache}/ait/makepkg/<仓库路径哈希>/`，安装包在其中的 `packages/`。
Cargo 的下载缓存沿用用户配置。可用 `AIT_MAKEPKG_DIR=/外部目录 makepkg -si` 更换构建位置；
路径指向仓库内部时会拒绝执行。`makepkg --packagelist` 可查看安装包的完整路径。

修改源码后同版本重建安装使用 `makepkg -sif`，避免复用之前的安装包。此配方依赖当前
Git 工作区，不用于 `makepkg --source` 或单独发布到 AUR；AUR 二进制配方见下节。
`packaging/` 是需要提交的打包源文件，不在 `.gitignore` 中。

## Arch Linux / AUR

[`packaging/aur/`](../../packaging/aur/) 保存两个 AUR 包的配方，由
[`aur-publish.sh`](../../scripts/aur-publish.sh) 发布。`ait-bin` 由
[`publish-aur.yml`](../../.github/workflows/publish-aur.yml) 自动发布，`ait-git` 由维护者手动发布：

- `ait-bin`：使用对应 GitHub Release 的 `Ait-linux-x86_64.AppImage`。正式版 Release 发布（`release: published`，
  跳过 beta 和 dev build）后自动把 `pkgver` 设为该版本、`pkgrel` 重置为 `1`，校验值取自 Release 的
  `SHA256SUMS`，`ait.desktop` 的校验值按文件重新计算。仓库内 `pkgver`、`pkgrel` 和校验值以 AUR 为准，
  无需手动修改。只在正式版 Release 发布或手动运行时执行，推送 tag 时 Release 仍是草稿，不触发；
  合并到 `main` 和 PR 都不运行。同版本修改配方后手动运行工作流，会先完整构建验证，再自动递增
  `pkgrel` 推送；不会发布比 AUR 更旧的版本。
- `ait-git`：从 `main` 源码构建，版本号由 `pkgver()` 在用户构建时生成，用户通过 AUR helper 的
  `--devel` 获取新提交。AUR 规定不能提交只改 `pkgver` 的 VCS 包更新，因此只有配方本身变化时才更新，
  由维护者在本机用自己的 AUR SSH key 运行 `build`（完整编译验证）后再 `publish`。

`ait-bin` 每次更新在 [`publish-aur.yml`](../../.github/workflows/publish-aur.yml) 中分两个 job 执行：

1. 构建：在 `archlinux:base-devel` 容器中完整运行 `makepkg`。构建会执行上游 npm/Cargo 依赖的代码，
   因此此 job 不接触 AUR 私钥。
2. 发布：构建成功后在另一台 runner 上运行，不构建，沿用构建 job 解析出的版本，只生成 `.SRCINFO`，提交并推送到 AUR。

`.SRCINFO` 不提交到本仓库。也可手动运行工作流，`tag` 留空时使用最新正式版。

AUR 私钥只保存在 GitHub environment `aur` 的 secret `AUR_SSH_PRIVATE_KEY` 中，不设仓库级 secret。
该 environment 只允许 `main` 分支和 `v*` 标签部署，其他分支上修改过的工作流取不到私钥；
fork PR 本就没有 secret。对应公钥需加入 AUR 维护者账户（My Account → SSH Public Key）。
本地运行（`AUR_PUSH=1` 时才推送；`publish` 只在配方或版本有变化时提交）：

```bash
AIT_RELEASE_VERSION=X.Y.Z  # 替换为已经发布的正式版本
scripts/aur-publish.sh build ait-bin "$AIT_RELEASE_VERSION"
scripts/aur-publish.sh publish ait-bin "$AIT_RELEASE_VERSION"
scripts/aur-publish.sh build ait-git
AUR_PUSH=1 scripts/aur-publish.sh publish ait-git
```

安装后可在应用菜单打开 Ait，或运行 `ait`；同时注册 `ait://` 链接。
此安装方式由 pacman / AUR helper 管理升级，不使用 AppImage 自动更新。

## 每日发布判断

从上一正式版附件 `BUILD-INFO.json` 的实际 `sourceCommit` 比较 `main`：新增功能和修复
都计入发布条件。已发布内容和未合并 PR 不重复纳入；只有版本、文档或构建记录变化时不自动
创建新版本。同版本重建可能保留旧标签，不能仅用上一版本标签作为已交付源码的判断基线。
有符合条件的变更时准备下一个补丁版本，并按下方完整发布流程自动完成正式发布。

## 准备版本

同步根 `Cargo.toml` 的 `workspace.package.version`、Cargo.lock、根 package.json、
所有活跃 npm workspace 及其 lockfile，并完成下方的 [iOS 原生版本同步](#ios-原生版本同步)。
即使本次只发布桌面，也必须在打标签前同步已提交的 iOS 工程，保证该标签可用于后续移动端构建。
然后运行：

```bash
npm ci
npm run verify:release
npm run test:release
npm run test:mobile-release
npm run build:desktop-main
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
```

更新根 CHANGELOG.md：该文件
会打入应用，供“新功能”页面读取。把版本与发布变更经 PR 合并到 `main` 后，再创建标签。

### iOS 原生版本同步

更新 `apps/mobile/package.json` 不会自动更新已提交的 iOS 工程。每次准备版本时，必须同步：

- `apps/mobile/ios/Ait/Info.plist` 的 `CFBundleShortVersionString` 和 `CFBundleVersion`，
  分别使用 `getNativeReleaseVersion(package.version)` 返回的 `appVersion` 和 `iosBuildNumber`
  （定义见 [`native-release-version.js`](../../apps/mobile/native-release-version.js)）。
  例如 `0.1.15` 对应应用版本 `0.1.15`、源码中的 build number `1015999`；beta 也按该函数推导。
  这是提交到 Git 的原生元数据；上传 TestFlight 的构建号由 Xcode Cloud 独立递增，见下文。
- `apps/mobile/ios/Podfile.lock` 中本地模块 `ExpoTwoWayAudio` 的版本与 podspec checksum。
  使用锁定的 CocoaPods 重新生成锁文件，不手工修改 checksum。

在 macOS 上安装根 npm 依赖后，以 Xcode Cloud 的生产应用身份运行 Expo prebuild，
再更新并验证 CocoaPods 锁文件。下列公开身份须与 `ci_post_clone.sh` 的默认值保持一致；
Ruby、Bundler 和 CocoaPods 环境见 [Xcode Cloud 依赖安装](apple-builds.md#xcode-cloud-依赖安装)。

```bash
(
  set -eu
  cd apps/mobile
  APP_VARIANT=production IOS_BUNDLE_IDENTIFIER=com.necokeine.ait \
    APPLE_TEAM_ID=SVS7GV79T9 CI=1 EXPO_NO_TELEMETRY=1 \
    env -u AIT_WEB_PLATFORM npm exec -- expo prebuild --platform ios --no-install
  cd ios
  bundle install
  bundle exec pod install
  bundle exec pod install --deployment
)
node --test scripts/xcode-cloud.test.mjs
```

核对生成的 plist 版本与上述推导值一致，将原生工程和锁文件随版本 PR 一起合并，再创建标签。
`verify:release` 当前只检查 Cargo/npm 版本，不覆盖这些 iOS 字段；Xcode Cloud 的
`ci_post_clone.sh` 会拒绝应用版本不一致的工程，后续 `pod install --deployment` 也要求锁文件一致。
旧标签不会包含后补的版本修复，重试同一提交无法消除这类错误，必须从包含修复的提交重新构建。

### 发布覆盖率与 Release Notes

每次 release 在正式发布前实际测量覆盖率，并写入 Release Notes。默认按本次变更涉及的
Rust crate 和相关测试测量；只有用户明确要求整个 Rust workspace 时才运行全工作区覆盖率。
Rust 测量使用 `cargo llvm-cov`，全工作区请求生成 HTML 报告：

```bash
cargo llvm-cov nextest --workspace --locked --html
```

Release Notes 的 `Test coverage` 一节记录测量源码提交、精确命令、范围与 features、
覆盖行数／总行数及百分比，并链接可共享的报告或证据。报告必须对应本次发布源码；准备后
若仅增加版本／文档，可用源码指纹说明等价关系。已有历史测量不能当作当前结果。
同时列明忽略用例、未测平台和重要未覆盖行为；测试通过数量与覆盖率分别报告。
若有可比基线，注明变化；没有则明确说明，不推算。JavaScript／TypeScript 与 Rust 的
测量范围分别说明，不把某个语言或定向文件的结果称为整个产品覆盖率。

### Beta 版本

桌面 beta 使用 `X.Y.Z-beta.N`（N 从 1 开始），例如 `0.0.23-beta.1`；Cargo、npm
workspace 和 lockfile 同步使用完整版本。创建不可变标签 `v0.0.23-beta.1`，由同一个桌面
工作流构建、签名、公证并执行成品门禁。

Electron 自动更新资产使用 `beta-linux.yml`、`beta-mac.yml`，不上传 `latest-*.yml`。
GitHub Release 创建和修复均使用 `--prerelease --latest=false`，保留正式版的 Latest 指向。
用户可以在桌面更新设置中选择 beta 通道，或从预发布下载页手动安装。

根 `PKGBUILD` 的 Arch 版本使用 `0.0.23beta1`，`_ait_version` 保留完整 SemVer 用于
源码校验；正式版准备时也要同步这两个字段。AUR 二进制配方继续引用已验证的正式版。
beta 标签不构建 Android APK，也不提交 Google Play 或启动 iOS TestFlight 构建。

## 创建 Release

确认版本 PR 已包含 [iOS 原生版本同步](#ios-原生版本同步)的工程与锁文件变更，并通过验证后再打标签。

```bash
git switch main
git pull --ff-only
npm run verify:release
AIT_RELEASE_TAG="v$(node -p "require('./package.json').version")"
git tag -a "$AIT_RELEASE_TAG" -m "Ait $AIT_RELEASE_TAG"
git push origin "$AIT_RELEASE_TAG"
```

`.github/workflows/release.yml` 构建 Linux x86_64、macOS arm64 和 Windows x64 桌面；
Windows 使用独立工作流，不构建 daemon。Linux/macOS 流程为：

1. 校验标签和全部活跃版本，安装根 npm workspace，验证发布脚本。
2. 用锁定依赖只构建 `daemon` 的 `daemon`；Linux 在 `manylinux_2_28` 容器中编译并检查 glibc 基线。
3. 导出界面、编译 Electron 主进程，验证 daemon 版本并暂存单个可执行文件。
4. 检查打包内容；macOS 签名、公证；隔离启动成品应用并验证真实 daemon 生命周期。
5. 收集桌面安装包和自动更新资产，核对更新摘要。
6. 将同一 daemon 打包为独立归档并核对其 `--version`；macOS 先单独签名、公证。
7. 桌面构建成功后汇总资产（必须包含两个独立 daemon 归档）并生成 SHA256SUMS，再创建 Draft
   Release 或补齐既有 Release 的资产。

新建 Release 使用 [`gh release create --draft`](https://cli.github.com/manual/gh_release_create)，
将安装包暂存为草稿，供代理继续校验资产、整理说明与覆盖率证据，然后自动完成正式发布。
草稿生成期间，公开 Latest 和正式通道自动更新继续使用上一正式版。
重跑可补齐既有草稿，包括首次上传失败后没有任何附件的空草稿；既有正式版的修复保持原发布状态。

### 自动完成正式发布

自 2026-10-17 起，用户说“开始 release”“准备新版本”或“发布新版本”，以及每日发布判断
满足条件时，代理默认执行完整发布流程。完成检查后直接 finalize，无需人工 review、
二次确认或再次要求用户发送“final release”。用户明确要求只准备 Draft 时，才保持草稿。

1. 等待适用的 CI、三平台桌面构建、macOS 签名/公证、Linux/macOS daemon 生命周期与 Windows 启动门禁通过。
2. 核对不可变标签、`BUILD-INFO.json` 的源码与工作流提交、完整附件、`SHA256SUMS` 和
   更新元数据；整理变更说明、兼容性限制与本次实测覆盖率证据并写入 Release Notes。
3. 检查通过后直接将草稿转为正式发布。任一检查失败或证据缺失时，先修复并重新验证，
   通过前保持 Draft。
4. 发布后重新确认非 Draft 状态、版本标签、附件与说明；稳定版确认 Latest 指向本次版本，
   再向用户提供正式链接、构建源码、验证结果及覆盖率报告。

稳定版完成上述发布前检查后直接使用：

```bash
gh release edit vX.Y.Z --repo OWNER/REPO --draft=false --latest
```

Beta 仍需保持 `--prerelease --latest=false`。现有工作流负责生成草稿和资产，代理自动衔接
后续校验、说明整理与正式发布。

macOS 需要 GitHub Secrets：`MAC_CSC_LINK`、`MAC_CSC_KEY_PASSWORD`、`APPLE_ID`、
`APPLE_BUILD_APP_SECRET`、`APPLE_TEAM_ID`。缺失签名、公证凭据会阻止发布；不会降级为未签名包。
Release Note 由 `.github/release.yml` 根据合并 PR 分组生成。

手动重跑：Actions → Release Ait → Run workflow，输入已存在的标签。工作流不会创建标签。
重跑会构建桌面，稳定标签还会再次提交 Google Play 内部测试并启动新的 iOS 内部测试构建；Release job 验证并保留已有的 Android APK，草稿状态下新的 APK 会替换它。
已有 Release 保留说明，覆盖同名资产；不要移动已经公开使用的标签。

默认应用源码从输入的发布标签检出；资产收集、校验和相关测试从工作流自身的提交检出到
`.tmp/release-tools`。因此修复发布工具后，可以选择包含修复的工作流分支重跑原始标签：

```bash
gh workflow run release.yml --ref main -f tag=vX.Y.Z
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
gh workflow run release.yml --ref YOUR_PR_BRANCH -f tag=vX.Y.Z -f source_commit=FULL_COMMIT_SHA
```

## 本地验证

Linux x86_64：

```bash
npm ci
cargo build --locked --release -p daemon --bin daemon --target x86_64-unknown-linux-gnu
AITD_BIN="$PWD/target/x86_64-unknown-linux-gnu/release/daemon" \
  AIT_DESKTOP_SMOKE=1 npm run package:linux
```

Linux 需安装 `xvfb`、FUSE 和 Electron 的系统库；具体包名见 workflow。

macOS arm64（正式签名、公证）：

```bash
npm ci
# 配置 CSC_NAME 或 CSC_LINK，以及 Apple 公证凭据后：
AIT_DESKTOP_SMOKE=1 npm run package:mac
```

`package:mac` 生成 DMG 与 ZIP；`package:linux` 生成 AppImage 与 tar.gz。省略 `AITD_BIN`
会从当前源码构建 release daemon；提供该变量时仍检查二进制版本。只允许原生目标平台、架构。

无签名凭据时，本机开发验证使用 `AIT_DESKTOP_SMOKE=1 npm run build:dmg`。它生成
`Ait-VERSION-local-arm64.dmg`，不属于正式发布文件，不能通过正式资产收集门禁。

输出位于 `apps/desktop/release/`，暂存输入位于 `apps/desktop/release-resources/daemon/`，均不提交。
下载后用 Linux `sha256sum -c SHA256SUMS` 或 macOS `shasum -a 256 -c SHA256SUMS` 校验。

## 兼容性与失败恢复

新应用沿用 Ait 正式应用 ID `dev.ait.desktop`。独立 daemon 默认使用
`~/.ait-server-desktop`；旧桌面的 SQLite 数据保留，但本次不自动迁移到新 daemon。

- 版本或包内容验证失败：修复 manifest、锁文件或暂存输入，重新运行门禁。
- 任一桌面平台构建、签名或启动失败：Release job 不会运行。Android Play 失败不在此列，
  需单独查看 `android-play` job。
- 发布阶段失败：在同一不可变标签上手动重跑，补齐资产。
- 已公开的错误版本：发布新的补丁版本，不移动标签。

## Google Play Android Internal Testing

每个稳定标签由 [Release Ait](../../.github/workflows/release.yml) 的 `android-play` job
发布给内部测试者，预发布标签跳过。在 GitHub runner 上使用 Gradle
和 Secrets 中的上传 keystore 构建签名 AAB，通过校验后直接调用 Play API 上传。
`versionCode` 使用迁移基数、Actions 运行编号与重试次数计算，不依赖 EAS。

首次服务账号配置、Play App Signing、测试者名单、安装链接和重试步骤统一见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。
Play 处理完成后，`android-apk` job 把 Google 签名的 universal APK 附加到草稿 Release。
发布边界见 [ADR-175](../decisions/clients/adr-175-android-play-gradle-release.md)、
[ADR-234](../decisions/clients/adr-234-play-internal-in-release-workflow.md) 和
[ADR-236](../decisions/clients/adr-236-google-signed-apk-release-asset.md)。

## App Store iOS 手动发布

在 Xcode 或 App Store Connect 手动启动 **Release iOS to App Store**，选择 `main` 或已完成
版本准备的稳定标签 `vX.Y.Z`。此工作流用于外部 TestFlight 与正式上架所需的归档。

| 设置                            | 期望值                                                                           |
| ------------------------------- | -------------------------------------------------------------------------------- |
| Product / Workflow              | `Ait` / `Release iOS to App Store`                                               |
| Repository / Workspace / Scheme | `ait-app/ait` / `apps/mobile/ios/Ait.xcworkspace` / `Ait`                        |
| Start Condition                 | 手动选择 Branch / Tag                                                            |
| Actions                         | `Build - iOS`；`Archive - iOS`，Distribution Preparation = **App Store Connect** |
| Post-Actions                    | Archive → `Ait Internal`；Archive → `Ait External`                               |

配置保存在 Apple 服务中，GitHub YAML 不会创建或修复它；启动前在
[Cloud 工作流](https://appstoreconnect.apple.com/teams/3e9381fe-a5bd-4fda-bc82-4f218c20480b/xcode-cloud/products/9153f73a-c52d-458b-ab3c-762228945ce5/workflows/d1dfac80-341e-4726-81e8-89202ac76a06)
核对。此工作流只手动启动，`release.yml` 不会触发它，见 [ADR-187](../decisions/clients/adr-187-ios-app-store-cloud-trigger.md)
和 [ADR-235](../decisions/clients/adr-235-ios-testflight-in-release-workflow.md)。
既有触发标签只用于追溯，不移动或强制覆盖。

上传后确认外部 TestFlight post-action、Apple 处理与测试组可用状态；Apple 可能要求 Beta App Review。
**正式 App Store 审核和审核后的发布仍手动操作。** 同一份符合分发要求的构建可用于外部测试和正式审核，
无需仅为上架重复编译。共同准备、依赖和重试要求见下节。

## Apple TestFlight iOS 内部测试

每个稳定标签由 `release.yml` 的 `ios-testflight` job 启动 Ait 的 **Default** 工作流，归档、签名和
内部分发由 Xcode Cloud 完成。job 成功只表示构建已启动，结果在
[App Store Connect](https://appstoreconnect.apple.com/apps/6817043477) 查看。预发布标签和带
`source_commit` 的重建跳过此 job。启动失败不影响 GitHub Release，也不改变整次运行的结论，
需查看该 job。日常用 `main` 测试时，仍可在 Xcode 或 App Store Connect 手动启动。

首次使用前，在 App Store Connect 的「用户和访问」→「集成」创建团队 API key，下载一次性的
`.p8` 私钥，然后配置 GitHub Secrets（文件不进入 Git、日志或聊天）：

```bash
gh secret set APP_STORE_CONNECT_ISSUER_ID --repo ait-app/ait
gh secret set APP_STORE_CONNECT_KEY_ID --repo ait-app/ait
gh secret set APP_STORE_CONNECT_PRIVATE_KEY --repo ait-app/ait < /absolute/path/AuthKey.p8
```

该 key 只用于启动构建，使用 **App Manager** 角色：它可完整使用 Xcode Cloud，Developer 只有只读权限，
Admin 权限超出需要。创建 key 需 Account Holder 或 Admin 登录。未配置时 job 报错，发布其余部分继续。

| 设置                            | 期望值                                                                                            |
| ------------------------------- | ------------------------------------------------------------------------------------------------- |
| Product / Workflow              | `Ait` / `Default`                                                                                 |
| Repository / Workspace / Scheme | `ait-app/ait` / `apps/mobile/ios/Ait.xcworkspace` / `Ait`                                         |
| Bundle ID / Apple Team          | `com.necokeine.ait` / `SVS7GV79T9`                                                                |
| Start Conditions                | `Manual Start - Branch, Tag`；分支为 `main`，标签手动选择                                         |
| Actions                         | `Build - iOS`；`Archive - iOS`，Distribution Preparation = **TestFlight (Internal Testing Only)** |
| Post-Actions                    | Archive → `TestFlight Internal Testing - iOS` → `Ait Internal`                                    |

仅 Build 成功不会上传 TestFlight；必须完成 Archive 与 post-action。内部专用归档不能直接用于正式上架，
需要上节的 App Store 分发构建。设计见 [ADR-183](../decisions/clients/adr-183-ios-xcode-cloud-release.md)。

### 共同准备与验收

1. 完成[版本准备](#准备版本)和 [iOS 原生版本同步](#ios-原生版本同步)，提交工程、共享 scheme 与锁文件。
2. 日常测试可选 `main`；正式版本的内部测试由标签自动启动，手动启动 App Store 构建时选择同一标签。
   核对 Cloud 所选 commit、Cargo/npm 版本及 Info.plist 一致，旧标签缺少修复时不能靠重试解决。
3. Cloud 的 [`ci_post_clone.sh`](../../apps/mobile/ios/ci_scripts/ci_post_clone.sh) 安装锁定依赖、校验版本、
   构建共享包和 Terminal WebView，以 `pod install --deployment` 准备归档。
   Cloud 不执行 Expo prebuild，也不修改或提交原生工程；环境见 [Apple 构建](apple-builds.md#xcode-cloud-依赖安装)。
4. 确认 Archive、上传和相应 TestFlight post-action 均完成，再检查 Apple 处理与测试组可用状态。
   构建成功不等于可以分发；所需合规信息在 App Store Connect 中完成。

### 构建号与重试

上传构建号使用 Xcode Cloud 的 `CI_BUILD_NUMBER`，与源码中 Expo 推导的 build number 分开。
不要重置 Cloud 计数器；若同一应用版本已有其他渠道上传的构建，先核对已用序号。

先查看失败 action 的 Logs。临时网络失败可重试同一提交；版本或锁文件不一致要先修复并提交，
再构建新提交。Archive 成功但测试不可用时，先查 post-action、处理与审核状态，避免重复构建。
发布权限、签名和上传由 Apple 团队及 Xcode Cloud 管理，不使用旧 EAS build ID 或 `eas submit`。

`eas.json` 只保留历史手动配置，当前 iOS CI 不使用 `EXPO_TOKEN` 或 GitHub 中的 Apple 上传凭据；
GitHub 中的 API key 只用于启动 Cloud 构建。重跑 `ios-testflight` job 会启动新的构建，失败时优先在 Cloud 内重试。
旧 Secret/environment 不随代码删除而消失，清理前确认没有其他使用者。Expo SDK、config plugins 和本地 prebuild 继续使用。
