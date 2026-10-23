# Ait 发布操作指南

从 0.0.7 起，GitHub Release 构建 `apps/desktop` Electron 桌面、`apps/mobile` 的 Web 导出和
Rust `daemon`。发布边界见 [ADR-053](../decisions/clients/adr-053-paseo-desktop-release.md)，
旧桌面源码的移除见 [当前架构](../architecture/README.md)。

推送标签和手动运行 Release Ait 构建 Linux、macOS 和 Windows 桌面；Android 使用独立的 Google Play AAB
工作流，在 GitHub runner 上用 Gradle 构建并直接上传 Play。签名 Secrets 与操作步骤见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。

`apps/mobile` 的 Google Play Android Internal Testing 发布流程见下方
[Google Play Android Internal Testing](#google-play-android-internal-testing) 一节，
与桌面 GitHub Release 相互独立。`apps/mobile` 的 Apple TestFlight 发布流程见下方
[Apple TestFlight iOS 手动发布](#apple-testflight-ios-手动发布)一节，
同样与桌面 GitHub Release 相互独立。可用于正式上架并自动提交外部测试的构建使用
[App Store iOS 手动发布](#app-store-ios-手动发布)。见
[ADR-183](../decisions/clients/adr-183-ios-xcode-cloud-release.md) 和
[ADR-187](../decisions/clients/adr-187-ios-app-store-cloud-trigger.md)。

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

## main CI Nightly

每次推送 `main` 都会通过 [CI](../../.github/workflows/ci.yml) 构建 Linux x86_64
和 Apple Silicon 安装包，适用的 CI 检查和两个平台构建全部成功后更新固定的 [nightly 预发布](https://github.com/ait-app/ait/releases/tag/nightly)。
无需创建版本标签；不再使用独立的 dev nightly 工作流。

nightly 的公开构建标识统一为 `<8位提交hash>-<提交UTC日期>`，例如 `38fb6054-2026-10-08`。
Release 标题为 `Ait Nightly 38fb6054-2026-10-08`，安装包名例如
`Ait-38fb6054-2026-10-08-macos-arm64.dmg`、`Ait-38fb6054-2026-10-08-linux-x86_64.AppImage`。
ZIP、tar.gz、blockmap、CI 产物和 `BUILD-INFO.json` 使用同一标识；两平台及重跑均取同一提交日期。
更新元数据沿用更新器识别的 `latest` / `beta` 入口文件名，内部引用对应的新安装包名。
Electron、daemon 及更新协议内部仍保留必需的 SemVer 兼容字段，在构建信息中记作 `packagedVersion`。
固定下载地址仍使用 `nightly` 标签；正式版本继续使用版本命名。

最多保留最新两次 main CI 并行运行，以整个工作流为单位（每次包含检查及 Linux、macOS 构建）；
取消旧构建也会取消该次旧 CI 中尚未完成的检查，PR 不参与这个调度窗口：

- 提交 1、2 相继触发构建时，保留两次构建。
- 1 先完成则先发布 1，2 完成后覆盖 1。
- 2 先完成并发布成功，则取消仍在运行的 1；晚完成的 1 不得覆盖 2。
- 提交 3 触发调度后取消 1，保留 2、3，依次沿用同一规则。

调度和发布共用串行锁，发布上传期间不会被调度取消；实际取消受 GitHub 调度和 API 响应时间影响。
构建本身不共用串行锁，两个平台全部成功后才能发布。发布队列使用 `queue: max`，避免默认的
单个等待位置让调度任务挤掉待发布任务。每次运行只清理自身的临时产物，不删除其他构建的文件。

以工作流首次运行的序号判断新旧，重跑不改变顺序。发布前同时校验最近两次运行和已发布序号，
允许较旧构建在较新构建未发布时先上线，但禁止 nightly 回退。Release 说明内记录已发布序号；
`BUILD-INFO.json` 记录源码提交、构建序号和构建链接，可用于确认安装包是否包含指定修复。
新提交编译期间或编译失败时仍保留上一版。
macOS nightly 使用 ad-hoc 签名，未公证。

nightly 同样附带独立 daemon 归档，例如 `Ait-38fb6054-2026-10-08-daemon-linux-x64.tar.gz`，
与桌面包共用同一次构建的 daemon。macOS 归档只有 ad-hoc 签名、未公证；用浏览器下载后若被拦截，
在解压目录执行 `xattr -d com.apple.quarantine ait-daemon`。见 [ADR-163](../decisions/clients/adr-163-nightly-standalone-daemon.md)。

## PR CI

创建或更新 PR 会运行适用的 Rust、UI、文档和格式检查。Linux x86_64 与 macOS Apple Silicon
桌面打包，以及 nightly 的准入、发布和产物清理任务均跳过。
影响桌面、共享界面、依赖或 Windows/Release 工作流的 PR 会运行
[Windows desktop](../../.github/workflows/windows-desktop.yml)：构建未签名 x64 NSIS 安装包和 ZIP，
实际启动打包客户端验证欢迎页及无内置 daemon，并保留 `ait-windows-x64` CI 产物 7 天。
该任务只有读取仓库的权限，不上传 GitHub Release。

合并到 `main` 后，由 main CI 构建并发布 nightly 安装包；合并前如需验证安装包，
可按[桌面端构建说明](../../apps/desktop/README.md)在本地打包。

Google Play 和 iOS TestFlight 不随 PR 或 main CI 构建，均使用各自的手动发布入口。
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
| Linux 独立 daemon | x86_64              | `Ait-VERSION-daemon-linux-x64.tar.gz`                                  |
| macOS 独立 daemon | Apple Silicon arm64 | `Ait-VERSION-daemon-macos-arm64.tar.gz`                                |
| Android           | 通用                | `Ait-VERSION-android.apk`                                              |
| 自动更新          | 各平台              | `latest-linux.yml`、`latest-mac.yml`、`latest.yml`、生成的 `.blockmap` |
| 构建来源          | Windows             | `BUILD-INFO-windows.json`                                              |
| 校验              | 全部资产            | `SHA256SUMS`                                                           |

AppImage 文件名保持稳定，版本体现在 Release 标签和应用内部。deb 与 rpm 的包名为
`ait-desktop`，安装到 `/opt/Ait`，并列入 `latest-linux.yml` 供原生更新器使用。Linux/macOS
安装包 `resources/bin/` 中只有 `daemon`；Windows 不附带 daemon，仅连接远程主机。
Windows Release 构建运行在 `windows-2025`，不安装 Rust 工具链、不构建独立 daemon。
未配置 Windows 代码签名证书时产物未签名。Electron 主程序与 Helper 是必需运行时。
GitHub 仍自动提供标签对应的源码归档。

向已发布版本追加 Windows 产物时，保持原标签和 Linux/macOS 文件不变；上传 EXE、ZIP、
更新 YAML、blockmap 与 `BUILD-INFO-windows.json`，在原 `SHA256SUMS` 中追加这些文件的校验和。
Windows 构建来源单独记录，原 `BUILD-INFO.json` 继续描述既有平台。Release CI 使用
`--windows` 强制要求完整 Windows 产物集；历史版本校验仍允许不含 Windows 的旧版本。

独立 daemon 归档供没有桌面环境的主机运行 [无人值守 daemon](headless-daemon.md)，与桌面包使用
同一次构建的 Rust `daemon`。归档内为可执行文件 `ait-daemon` 和 `LICENSE`；文件改名是为了放入
`PATH` 时不与系统同名程序冲突，`--help` 中的程序名仍为 `daemon`。macOS 版本单独用 Developer ID
签名（hardened runtime）并公证；裸可执行文件无法 staple 公证票据，Gatekeeper 首次运行时在线核验。
Linux daemon 在 `ubuntu-24.04` runner 上通过 [`build-linux-daemon.sh`](../../scripts/build-linux-daemon.sh) 于 `manylinux_2_28` 容器内编译，桌面包与独立归档共用；构建后检查不超过 `GLIBC_2.28` 与 `GLIBCXX_3.4.25`，支持 Ubuntu 20.04、Debian 10、RHEL 8 及更新版本。
正式版、beta 和 nightly 都包含这两个归档；nightly 版本见上文 [main CI Nightly](#main-ci-nightly)。

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
  跳过 beta 和 nightly）后自动把 `pkgver` 设为该版本、`pkgrel` 重置为 `1`，校验值取自 Release 的
  `SHA256SUMS`，`ait.desktop` 的校验值按文件重新计算。仓库内 `pkgver`、`pkgrel` 和校验值以 AUR 为准，
  无需手动修改。只在正式版 Release 发布或手动运行时执行，推送 tag 时 Release 仍是草稿，不触发；
  合并到 `main` 和 PR 都不运行。同版本修改配方后手动运行工作流，会先完整构建验证，再自动递增
  `pkgrel` 推送；不会发布比 AUR 更旧的版本。
- `ait-git`：从 `main` 源码构建，版本号由 `pkgver()` 在用户构建时生成，用户通过 AUR helper 的
  `--devel` 获取新提交。AUR 规定不能提交只改 `pkgver` 的 VCS 包更新，因此只有配方本身变化时才更新，
  由维护者在本机用自己的 AUR SSH key 运行 `build`（完整编译验证）后再 `publish`。

`ait-bin` 每次更新分两个 job，由 [`aur-package.yml`](../../.github/workflows/aur-package.yml) 执行：

1. 构建：在 `archlinux:base-devel` 容器中完整运行 `makepkg`。构建会执行上游 npm/Cargo 依赖的代码，
   因此此 job 不接触 AUR 私钥。
2. 发布：构建成功后在另一台 runner 上运行，不构建，只生成 `.SRCINFO`，提交并推送到 AUR。

`.SRCINFO` 不提交到本仓库。也可手动运行工作流，`tag` 留空时使用最新正式版。

AUR 私钥只保存在 GitHub environment `aur` 的 secret `AUR_SSH_PRIVATE_KEY` 中，不设仓库级 secret。
该 environment 只允许 `main` 分支和 `v*` 标签部署，其他分支上修改过的工作流取不到私钥；
fork PR 本就没有 secret。对应公钥需加入 AUR 维护者账户（My Account → SSH Public Key）。
本地运行（`AUR_PUSH=1` 时才推送；`publish` 只在配方或版本有变化时提交）：

```bash
scripts/aur-publish.sh build ait-bin 0.1.9
scripts/aur-publish.sh publish ait-bin 0.1.9
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
npm run verify:release -- v0.0.20
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
beta 标签不构建 Android APK；Google Play、iOS TestFlight 仍通过各自的手动入口发布，不随 beta 标签触发。

## 创建 Release

确认版本 PR 已包含 [iOS 原生版本同步](#ios-原生版本同步)的工程与锁文件变更，并通过验证后再打标签。

```bash
git switch main
git pull --ff-only
npm run verify:release -- v0.0.20
git tag -a v0.0.20 -m "Ait v0.0.20"
git push origin v0.0.20
```

`.github/workflows/release.yml` 在 Linux x86_64 和 macOS arm64 原生 runner 上构建桌面：

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

1. 等待适用的 CI、两平台构建、macOS 签名、公证和成品生命周期门禁通过。
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
重跑只构建桌面；如 Release 已有 Android APK，则验证并保留该附件。
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

Android Play 内部测试使用独立手动工作流
[Release Android to Google Play](../../.github/workflows/release-android-play.yml)，
支持只构建 AAB、上传内部测试草稿或发布给内部测试者。在 GitHub runner 上使用 Gradle
和 Secrets 中的上传 keystore 构建签名 AAB，通过校验后直接调用 Play API 上传。
`versionCode` 使用迁移基数、Actions 运行编号与重试次数计算，不依赖 EAS。

首次服务账号配置、Play App Signing、测试者名单、安装链接和重试步骤统一见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。
独立的 GitHub APK 工作流已移除，桌面 Release 不再调用 Android 构建。发布边界见
[ADR-175](../decisions/clients/adr-175-android-play-gradle-release.md)。

## App Store iOS 手动发布

GitHub Actions → **Release iOS to App Store** → **Run workflow** 会触发 Xcode Cloud
归档、上传 App Store Connect，并自动提交到 `Ait External` 外部 TestFlight 测试组。
Apple 可能要求 Beta App Review；通过后才可向外部测试者分发。**正式 App Store 审核仍手动
提交，审核通过后的发布也保留手动。** 同一份 App Store Connect 构建可用于外部测试和正式
审核，不必为了正式上架再次编译。

### GitHub 入口

工作流文件为 [release-ios-app-store.yml](../../.github/workflows/release-ios-app-store.yml)。

1. 按[准备版本](#准备版本)同步稳定版本及 [iOS 原生版本](#ios-原生版本同步)，提交工程和锁文件。
2. 在 Run workflow 选择包含入口的分支（通常为 `main`）。`tag` 留空时使用该次 dispatch
   固定的源码 commit；填写 `vX.Y.Z` 时使用该稳定标签的 commit，不是之后变化的分支 HEAD。
3. 入口校验 Cargo/npm/workspace 锁文件及 `Info.plist` 版本一致；旧 `v0.1.15` 标签的原生
   版本不一致会在触发 Cloud 前失败。发布辅助代码取自运行工作流的 commit，不执行源码标签内的脚本。
4. GitHub 只推送一个 `ios-app-store/vX.Y.Z/<run-id>-<attempt>` 标签，然后等待 Cloud
   新 Build/Archive checks。运行摘要记录源码 SHA、触发标签、Cloud 构建链接和外部测试组链接。
5. 到 Cloud 确认上传和外部 TestFlight post-action，再到 TestFlight 确认处理、审核及组内
   可用状态。GitHub 绿色只确认 Build/Archive 成功，不能当作外部测试或正式审核已通过。
6. 正式上架时，在 App Store 版本页面选择已处理完成的同一构建，补齐资料后手动提交审核。

此入口不依赖桌面 GitHub Release 已完成。它只支持 `workflow_dispatch`，不会因普通 push、
PR、桌面 release 或 `vX.Y.Z` 标签自动启动，也不创建桌面 Release。需要 GitHub 仓库运行
Actions 的权限；使用临时 `GITHUB_TOKEN` 的 `contents: write` 推送标签和 `checks: read`
读取结果，无需额外 Apple secret。

### App Store Cloud 配置

以下配置已保存在 Apple 服务中；GitHub YAML 不会自动创建或修复它。

| 设置                            | 值                                                                               |
| ------------------------------- | -------------------------------------------------------------------------------- |
| Product / Workflow              | `Ait` / `Release iOS to App Store`                                               |
| Workflow ID                     | `d1dfac80-341e-4726-81e8-89202ac76a06`                                           |
| Repository / Workspace / Scheme | `ait-app/ait` / `apps/mobile/ios/Ait.xcworkspace` / `Ait`                        |
| 自动 Start Condition            | Tag Changes → Custom Tags → **Tags beginning with `ios-app-store/`**             |
| 手动 Start Condition            | 保留 `main` 分支和 Tag 的手动启动入口                                            |
| Actions                         | `Build - iOS`；`Archive - iOS`，Distribution Preparation = **App Store Connect** |
| Post-Actions                    | Archive → `Ait Internal`；Archive → `Ait External`                               |
| Editing                         | Account Holder / Admin / App Manager（Apple 对外部测试的要求）                   |

[Cloud 工作流](https://appstoreconnect.apple.com/teams/3e9381fe-a5bd-4fda-bc82-4f218c20480b/xcode-cloud/products/9153f73a-c52d-458b-ab3c-762228945ce5/workflows/d1dfac80-341e-4726-81e8-89202ac76a06)
复用现有原生工程和 `ci_post_clone.sh`。Tag Changes 只匹配专用前缀；
参见 [Apple 的标签发布工作流示例](https://developer.apple.com/videos/play/wwdc2023/10278/)。
`Default` 继续用于仅内部测试，`Public TestFlight` 的既有手动入口也保留。

触发标签保留用于追溯，不要移动或强制覆盖。GitHub 重跑会创建新标签和新 Cloud 构建，
因此上传/外部审核仍在处理时应先检查远端状态。GitHub 最多等待 15 分钟发现新 Cloud 构建、
90 分钟等待归档；超时或取消 GitHub 运行**不会取消 Cloud**。若从未出现 Cloud checks，
检查标签前缀、工作流启用状态及 Xcode Cloud GitHub App 连接；若归档失败，先看 Cloud 日志。
避免在同一提交上同时手动运行这条 Cloud 工作流，否则多个新构建会导致结果关联报错。

## Apple TestFlight iOS 手动发布

`apps/mobile` 的 iOS CI 统一使用 **Xcode Cloud**：直接在 Xcode 或
[App Store Connect](https://appstoreconnect.apple.com/apps/6817043477) 启动 Ait 的 `Default`
工作流，由 Apple 完成归档、签名、上传和 TestFlight 内部分发。
与桌面 GitHub Release、Google Play Internal Testing 独立，保持手动触发。
完整设计见 [ADR-183](../decisions/clients/adr-183-ios-xcode-cloud-release.md)。

### 云端工作流配置

工作流配置保存在 Xcode Cloud，由 Xcode 的 Edit Workflow 或 App Store Connect 管理；
GitHub YAML 无法替代这些设置。维护时核对：

| 设置                                | 值                                                                                    |
| ----------------------------------- | ------------------------------------------------------------------------------------- |
| Product / Workflow                  | `Ait` / `Default`                                                                     |
| Repository                          | `https://github.com/ait-app/ait.git`                                                  |
| Workspace / Scheme                  | `apps/mobile/ios/Ait.xcworkspace` / `Ait`                                             |
| Bundle ID / Apple Team              | `com.necokeine.ait` / `SVS7GV79T9`                                                    |
| Start Conditions                    | `Manual Start - Branch, Tag`：分支限定 `main`，标签允许手动选择；不添加自动触发       |
| Actions                             | 保留 `Build - iOS`；必须包含 `Archive - iOS`，scheme 为 `Ait`                         |
| Archive 的 Distribution Preparation | `TestFlight (Internal Testing Only)`                                                  |
| Post-Actions                        | `TestFlight Internal Testing - iOS`，使用 `Archive - iOS` 产物，测试组 `Ait Internal` |

仅 `Build - iOS` 成功不会上传 TestFlight。必须完成 Archive，并由 TestFlight post-action
分发；这是 [Apple 官方分发工作流](https://developer.apple.com/documentation/xcode/creating-a-workflow-that-builds-your-app-for-distribution)
要求的配置。内部测试构建不能直接转为正式 App Store 发布产物；正式上架使用上方
[App Store iOS 手动发布](#app-store-ios-手动发布)准备符合分发要求的归档。

仓库负责提交原生工程、共享 scheme、workspace、Ruby/Pod/npm 锁文件，以及
[`ci_post_clone.sh`](../../apps/mobile/ios/ci_scripts/ci_post_clone.sh)。该 hook 安装锁定依赖、
校验原生应用版本、构建共享包和 Terminal WebView，再以 `pod install --deployment`
准备归档依赖；详情见 [Xcode Cloud 依赖安装](apple-builds.md#xcode-cloud-依赖安装)。
Cloud 不执行 Expo prebuild，不自动修改或提交原生工程。

### 触发和验收

1. 先完成[准备版本](#准备版本)和 [iOS 原生版本同步](#ios-原生版本同步)，将工程与锁文件
   合并到 `main`。日常内部测试可手动选择包含修复的 `main` 提交。
2. 对应正式版本发布时，先完成该稳定 `vX.Y.Z` 的桌面 GitHub Release，再在 Cloud 手动选择
   同一个标签。手动 Start Conditions 已启用 Tag 选择；恢复或重建工作流时也须启用，
   不要添加标签推送的自动触发条件。启动前核对选中标签和源码 commit。
3. 在 Xcode Report navigator → Cloud → Ait → Default → Start Build，或在 App Store
   Connect 的 Xcode Cloud 中手动启动。稳定标签须包含原生版本修复；旧 `v0.1.15` 标签的
   原生版本不一致问题不会因为重试而消失，也不要移动已发布标签来绕过它。
4. 检查 `Archive - iOS` 成功、`TestFlight Internal Testing - iOS` 为 Completed，再到
   App Store Connect 的 TestFlight 确认版本、构建号和 `Ait Internal` 可用状态。构建成功、
   Apple 处理完成和测试组可用是不同阶段；需要处理的合规信息在 App Store Connect 中完成。

迁移后不再有 GitHub `ios-testflight` environment 审核、`gh release view` 自动门禁、
EAS 重复构建拦截或 `build_id` 重提入口。发布权限由 Apple 团队和 Xcode Cloud 管理；
稳定版本与 GitHub Release 的对应关系在手动启动前核对，不能假定 Cloud 会执行旧门禁。

### 构建号与重试

App Store Connect 使用 Xcode Cloud 递增的构建号（`CI_BUILD_NUMBER`），例如 Cloud 的
Build 8 对应 TestFlight `0.1.15 (8)`，不使用源码里由 Expo 推导的 `1015999` 作为上传序号。
参见 [Apple 的构建号说明](https://developer.apple.com/documentation/xcode/setting-the-next-build-number-for-xcode-cloud-builds)。
不要重置 Cloud 计数器；若同一应用版本已有其它渠道上传的构建，迁移前核对版本与构建号，
必要时在 Xcode Cloud Settings → Build Number 设置下一个可用值。

失败时先打开失败 action 的 Logs。仅网络拉取等临时错误可对相同提交重试；版本或锁文件
不一致必须先修复并提交，再构建包含修复的新提交。Archive 成功但 TestFlight 尚不可用时，
先检查 post-action 和 App Store Connect 的处理状态，避免在 Apple 仍处理产物时重复构建。
Cloud 的重试在 Cloud 界面操作，不使用旧 EAS build ID 或 `eas submit`。

### EAS 退出 CI

已移除 `.github/workflows/release-ios-testflight.yml`、EAS 专用发布辅助脚本和旧
`.eas/workflows` 发布／审核入口。iOS CI 不再需要 `EXPO_TOKEN`，也不向 GitHub 配置
App Store Connect API key、Apple ID 密码或签名证书；签名与上传由 Xcode Cloud 管理。
GitHub 中旧 environment 和 Secret 不会随代码删除而自动消失，只有确认无其它使用者后才清理。

Expo SDK、config plugins、本地 prebuild 和 `eas.json` 中的历史手动配置仍然保留，
它们不再承担 Ait 的 iOS CI 发布。外部 TestFlight 测试和 App Store 审核由维护者另行决定，
不属于当前内部测试工作流。
