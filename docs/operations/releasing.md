# Ait 发布操作指南

从 0.0.7 起，GitHub Release 构建 `apps/desktop` Electron 桌面、`apps/mobile` 的 Web 导出和
Rust `daemon`。发布边界见 [ADR-053](../decisions/clients/adr-053-paseo-desktop-release.md)，
旧桌面源码的移除见 [当前架构](../architecture/README.md)。

推送稳定标签默认只构建桌面；明确要求 Android APK 时，手动运行 Release Ait 并开启
`build_android`，在 GitHub runner 上用 Gradle 构建，也可独立生成测试包。
签名 Secrets 与操作步骤见 [Android APK 构建](android-releases.md)。
Android 商店构建使用下方的 Google Play AAB 工作流。

`apps/mobile` 的 Google Play Android Internal Testing 发布流程见下方
[Google Play Android Internal Testing](#google-play-android-internal-testing) 一节，
与桌面 GitHub Release 相互独立。`apps/mobile` 的 Apple TestFlight 发布流程见下方
[Apple TestFlight iOS 手动发布](#apple-testflight-ios-手动发布)一节，
同样与桌面 GitHub Release 相互独立。见 [ADR-070](../decisions/clients/adr-070-ios-testflight-release.md)。

## macOS 自动更新来源

新 macOS 安装包内嵌的更新源为 `https://aitapp.cn/api/v1/updates/macos/`，
稳定版和 beta 分别读取 `latest-mac.yml`、`beta-mac.yml`。AIT Server 提供清单及按地区
跳转的版本化文件入口；国内及未知地区下载 COS，其他已知地区下载 GitHub。
Linux 继续使用 GitHub 更新源。见 [ADR-165](../decisions/clients/adr-165-macos-ait-server-updates.md)。

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

合并到 `main` 后，由 main CI 构建并发布 nightly 安装包；合并前如需验证安装包，
可按[桌面端构建说明](../../apps/desktop/README.md)在本地打包。

Android APK、Google Play 和 iOS TestFlight 不随 PR 或 main CI 构建；APK 可手动运行
[Build Android APK](android-releases.md#手动构建)。main/PR CI 不调用 EAS 构建或提交，
不消耗移动端发布配额。

## 发布产物

| 平台              | 架构                | 文件                                                     |
| ----------------- | ------------------- | -------------------------------------------------------- |
| Linux             | x86_64              | `Ait-linux-x86_64.AppImage`                              |
| Linux             | x86_64              | `Ait-VERSION-linux-x64.tar.gz`                           |
| Linux（deb）      | x86_64              | `Ait-VERSION-linux-amd64.deb`                            |
| Linux（rpm）      | x86_64              | `Ait-VERSION-linux-x86_64.rpm`                           |
| macOS             | Apple Silicon arm64 | `Ait-VERSION-macos-arm64.dmg`                            |
| macOS             | Apple Silicon arm64 | `Ait-VERSION-macos-arm64.zip`                            |
| Linux 独立 daemon | x86_64              | `Ait-VERSION-daemon-linux-x64.tar.gz`                    |
| macOS 独立 daemon | Apple Silicon arm64 | `Ait-VERSION-daemon-macos-arm64.tar.gz`                  |
| Android           | 通用                | `Ait-VERSION-android.apk`                                |
| 自动更新          | 各平台              | `latest-linux.yml`、`latest-mac.yml`、生成的 `.blockmap` |
| 校验              | 全部资产            | `SHA256SUMS`                                             |

AppImage 文件名保持稳定，版本体现在 Release 标签和应用内部。deb 与 rpm 的包名为
`ait-desktop`，安装到 `/opt/Ait`，并列入 `latest-linux.yml` 供原生更新器使用。Windows 和其他架构
不属于本次发布。安装包 `resources/bin/` 中只有 `daemon`；Electron 主程序与
Helper 是必需运行时。GitHub 仍自动提供标签对应的源码归档。

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
所有活跃 npm workspace 及其 lockfile。然后运行：

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
重跑默认只构建桌面；显式开启 `build_android` 才重新构建 Android APK。
APK 构建失败时验证并保留此前已发布的 APK。
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
支持只构建 AAB、上传内部测试草稿或发布给内部测试者。它使用 EAS 托管上传签名，
在 Play runner 中启用远端递增 `versionCode`，并在提交前验证构建来源及 AAB。

首次服务账号配置、Play App Signing、测试者名单、安装链接和重试步骤统一见
[Google Play 内部测试与 CI 发布](google-play-internal-testing.md)。
请使用其中的 `play-internal` / `play-internal-draft` 提交 profile 和确定的 build ID。
独立的 GitHub APK 工作流已移除，遗留 EAS production workflow 不用于本流程。发布边界见
[ADR-097](../decisions/clients/adr-097-google-play-internal-release.md)。

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
