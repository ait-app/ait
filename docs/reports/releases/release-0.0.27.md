# Ait 0.0.27 发布说明

日期：2026-10-11（Asia/Shanghai）。发布准备基于 main `bd079ecd7e67f4c645ebbb6a24f13b315c6a650d`。
最终源码以不可变标签 `v0.0.27` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **退出后继续运行。** Host 设置可开启退出后保留内置 daemon，Agent 与终端继续运行；
  下次启动经过认证和 PID 校验后接管原 daemon。默认关闭，交接失败时仍停止进程。
  见 [PR #273](https://github.com/ait-app/ait/pull/273)、
  [ADR-128](../../decisions/clients/adr-128-desktop-daemon-keep-running.md)。
- **OpenCode 会话恢复。** 修复 ACP 工具投影、配置恢复、输入顺序、轮次身份与原生失败原因，
  避免会话元数据污染原生路由别名，终态前核对原生历史。
  见 [PR #252](https://github.com/ait-app/ait/pull/252)、
  [PR #255](https://github.com/ait-app/ait/pull/255)、
  [PR #256](https://github.com/ait-app/ait/pull/256)、
  [PR #258](https://github.com/ait-app/ait/pull/258)。
- **登录与在线主机。** 客户端仅保留浏览器统一登录，支持仅手机号账户；跨重启保存中继登录凭据
  与在线主机记录，登录后自动导入全部在线服务主机，保留已有本机连接。
  见 [PR #261](https://github.com/ait-app/ait/pull/261)、
  [PR #263](https://github.com/ait-app/ait/pull/263)、
  [PR #266](https://github.com/ait-app/ait/pull/266)、
  [PR #267](https://github.com/ait-app/ait/pull/267)、
  [PR #271](https://github.com/ait-app/ait/pull/271)。
- **Workspace 与 Timeline。** 首屏按身份读取目标 Workspace，完整目录后台同步；
  Timeline 通过持久化展示索引按页读取，减少长会话翻页的历史扫描。
  见 [PR #272](https://github.com/ait-app/ait/pull/272)。
- **客户端与运行效率。** Codex Auto-review 预批准 computer-use 工具并尊重显式覆盖；
  修复 Markdown 文件链接解码、未支持文件的预览布局与 DOM 终端等宽字体，补齐用量翻译，
  移除旧帮助链接和废弃配对 RPC，减少 daemon 周期性扫描与复制。
  见 [PR #265](https://github.com/ait-app/ait/pull/265)、
  [PR #245](https://github.com/ait-app/ait/pull/245)、
  [PR #260](https://github.com/ait-app/ait/pull/260)、
  [PR #268](https://github.com/ait-app/ait/pull/268)、
  [PR #264](https://github.com/ait-app/ait/pull/264)、
  [PR #274](https://github.com/ait-app/ait/pull/274)、
  [PR #253](https://github.com/ait-app/ait/pull/253)。

## 安装与兼容性

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.27)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.27-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.27-macos-arm64.dmg`、`Ait-0.0.27-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。macOS 签名、公证与两平台成品 daemon
生命周期门禁通过后上传安装包、`latest-linux.yml`、`latest-mac.yml`、`BUILD-INFO.json`
和 `SHA256SUMS`。

Timeline 数据库升级为 schema v6，旧 daemon 无法读取升级后的数据库；旧 Agent 首次访问
仍需建立索引。见 [ADR-125](../../decisions/providers/adr-125-durable-timeline-projection-pages.md)。
浏览器统一登录需要在线服务提供对应能力，客户端不再回退旧密码登录。

根 `PKGBUILD` 源码配方同步为 `0.0.27-1`。Android APK、Google Play、iOS TestFlight
和 AUR 二进制包继续使用独立流程。

## CI 修复与发布准备验证

准备基线 [main CI](https://github.com/ait-app/ait/actions/runs/38081126224) 的全仓 Oxfmt
检查失败，唯一不符合格式的文件为 `docs/reports/daemon/paseo-websocket-surface-phase-2.md`。
本次按固定 Oxfmt 版本修正该报告，保留全仓检查和 nightly 发布门禁。

本地验证：

- `npm ci --no-audit --no-fund`、版本一致性与 6 个本地 workspace 检查通过；
  Cargo 和 npm 锁文件只更新本地包版本，第三方依赖保持一致。
- `npm run test:release` 46 项、`npm run test:mobile-release` 41 项通过。
- 桌面主进程构建、桌面界面导出、桌面与移动端类型检查通过。
- 全仓 Oxfmt 检查通过，共 3252 个文件；全仓 Oxlint 为 0 个错误，保留 Playwright fixture
  的 2 个既有空解构警告。315 份 Markdown 本地链接验证通过。
- `cargo metadata --locked --offline`、`cargo fmt --all --check`、`bash -n PKGBUILD`
  和 `git diff --check` 通过。

发布 PR 的全部 CI、合并后的 main CI 与正式发布工作流全部通过后，才确认正式安装包发布完成。

## Test coverage

**Not measured。** 本次只修改发布版本、锁文件与 Markdown，没有修改 `bins/` 或 `crates/`
中的 Rust 源码，按仓库规则跳过本地 Rust 测试与覆盖率重测。发布测试数量与覆盖率分别记录。
功能变更的既有验证见对应 PR；本次由发布 PR、main CI 和两平台正式成品门禁验证最终源码。
