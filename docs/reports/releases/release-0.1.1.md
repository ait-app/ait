# Ait 0.1.1 Draft 发布说明

日期：2026-10-12（Asia/Shanghai）。发布准备基于 main `7e874cf6007a24be23de1551c325211fcdad0e9e`。
本次交付停在 Draft Release 等待 review；公开 Latest 继续保持 `v0.1.0`。
草稿安装包来源以标签 `v0.1.1` 与附件 `BUILD-INFO.json` 为准。

## 更新内容

- **Workspace 定时任务。** 任务固定归属于 Workspace，可使用现存 Agent 或每次新建 Agent。
  新列表按下一次触发时间与日期分组，统一创建、编辑和历史入口；全局创建按 Host、Project、
  Workspace 选择，默认运行一次，支持间隔、Cron、截止时间和次数上限。
  旧任务保留显示与兼容行为；具体规则见 [操作说明](../../operations/schedules.md)。
  对应 [PR #297](https://github.com/ait-app/ait/pull/297)、
  [PR #299](https://github.com/ait-app/ait/pull/299)、
  [PR #301](https://github.com/ait-app/ait/pull/301)。
- **实验性 Workspace Bot。** 在设置的“关于”开启默认关闭的测试功能后，可创建 Bot，
  使用内置对话、审批、取消、配置、共享记忆和归档；输入回执与历史持久恢复。
  后端支持 Telegram 长轮询与飞书长连接。外部 Channel 管理界面、流式输出与 Email
  仍属于后续功能。关闭客户端开关不暂停服务器 Bot。
  对应 [PR #296](https://github.com/ait-app/ait/pull/296)、
  [PR #303](https://github.com/ait-app/ait/pull/303)、
  [ADR-141](../../decisions/clients/adr-141-experimental-workspace-bots.md)。
- **无人值守 Host。** Linux/macOS daemon 提供登录、headless 运行、状态与退出，
  持久恢复机器授权、自动重连及本地 API 凭据。安装与恢复规则见
  [操作说明](../../operations/headless-daemon.md)。
  对应 [PR #195](https://github.com/ait-app/ait/pull/195)。
- **Codex 大结果与工具展示。** 超大 MCP 参数、结果与错误使用有界展示预览，完整值保留在
  原生 transcript，避免 Timeline 超限导致会话失败；批量 Read 不再重复复合命令的完整输出。
  对应 [PR #300](https://github.com/ait-app/ait/pull/300)、
  [PR #294](https://github.com/ait-app/ait/pull/294)。
- **连接与内部清理。** 前台 GitHub clone 校验期间保持连接；Relay 日志增加有界、脱敏的
  会话关联和断连原因，辅助定位问题，并不宣称已修复特定网络事件。
  清理已移除功能的协议和 SDK，合并重复模块与 helpers。
  对应 [PR #302](https://github.com/ait-app/ait/pull/302)、
  [PR #298](https://github.com/ait-app/ait/pull/298)、
  [PR #295](https://github.com/ait-app/ait/pull/295)、
  [PR #290](https://github.com/ait-app/ait/pull/290)。

## 安装与 review 范围

Draft 仅供有权限的 reviewer 查看，不进入正式自动更新。预期附件为 Linux x86_64 的
AppImage 与 tar.gz、macOS Apple Silicon 的 DMG 与 ZIP，以及更新元数据、blockmap、
`BUILD-INFO.json` 和 `SHA256SUMS`，共 10 个文件。macOS 要求 13 或更新版本。

Bot 功能需要 Host 公告 `workspace-bots-v1` 及完整方法清单，旧 Host 显示升级提示；
使用新的 Workspace 定时任务前也应更新目标 daemon。实验性 Bot 开关默认关闭。
OpenCode 2.x 最低支持 2.0.26，1.x 支持沿用原行为。

无人值守 Host 需要支持 `hostname` 注册字段和 `ait-daemon` client_id 的配套中心版本。
本任务没有部署中心或使用真实生产账号验收授权，也没有配置真实 Telegram／飞书凭据。
移动端商店、独立 Android APK 与 AUR 二进制包继续使用各自发布流程。

## 发布准备与门禁

Rust workspace 和 15 个本地 Cargo 包、根 npm 包及 6 个 workspace、锁文件与根
`PKGBUILD` 同步为 `0.1.1`；第三方依赖保持发布准备基线的锁定记录。

发布工作流新建 Release 时始终使用 `--draft`，重跑上传保留草稿状态；空草稿可直接补齐附件。
回归验证执行实际 Bash 步骤并使用隔离的 gh 替身，检查 stable／beta Draft、既有 Draft、
既有正式版修复、缺失标签及空草稿重试。它不访问 GitHub，也不调用真实发布命令。

本地验证通过：

- 锁定依赖安装、版本一致性与 6 个本地 workspace 链接检查。
- `npm run test:release` 53 项，包括 6 项 Draft 门禁回归；移动发布脚本 41 项。
- 桌面主进程构建、桌面与移动端类型检查。
- 全仓 Oxfmt 检查，共 3321 个文件；变更脚本／配置 lint 无错误与警告。
- 文档检查的 1 项测试与 348 份 Markdown 本地链接验证。
- `cargo metadata --locked --offline`、`cargo fmt --all --check`、`bash -n PKGBUILD`
  与 `git diff --check`；两份锁文件的第三方依赖记录与基线一致。

PR 和 main CI 门禁成功后创建标签，正式构建完成 macOS 签名、公证与两平台成品生命周期检查，
上传全部安装包并生成校验清单。核对 Draft 状态、附件摘要、更新元数据、构建来源和公开
Latest 后停下，等待用户 review；不执行发布草稿的命令。

## Test coverage

**Not measured。** 用户未请求覆盖率。本次发布准备只修改版本、锁文件、发布工作流、
相关脚本测试和文档，没有修改 `bins/` 或 `crates/` 的 Rust 源码，按仓库规则跳过本地
Rust 测试与覆盖率。既有 CI 门禁继续自动执行，历史功能验证不代表本次源码的覆盖率。
若后续请求测量，选择相关 crate 和定向测试并提供可共享报告；工作区覆盖率另需明确请求。
