# Ait 0.1.2 Draft 发布说明

日期：2026-10-13（Asia/Shanghai）。发布准备基于 main `a5a0f781de63027d049d73c9ed2ce4aee3e2610d`。
本次交付停在 Draft Release 等待用户 review，公开 Latest 继续为 `v0.1.1`。
最终安装包来源以标签 `v0.1.2` 和附件 `BUILD-INFO.json` 为准。

## 更新内容

- **图片输入。** 对齐单 Agent 的持久化接纳预算与既有合法输入上限，修复空队列也拒绝大图片的问题。
  客户端仅在单图或完整请求超出发送字节预算时压缩／生成可读细节分片，未超限图片保留原始字节；
  保存的原图和预览不被替换，无法满足上限时保留草稿并明确失败。
  见 [PR #310](https://github.com/ait-app/ait/pull/310) 与
  [设计及定向验证](../providers/image-input-admission-2026-10-13.md)。
- **OpenCode 与 Provider 错误。** 恢复 OpenCode 2.0.25 支持，正确保留长／多行 shell 命令和
  大文件编辑审批；初始配置失败时清理新建空会话，恢复失败时保留原历史。
  Provider 错误使用安全说明，失败提示写入持久化 Timeline，辅助排查和重启后查看。
  见 [PR #308](https://github.com/ait-app/ait/pull/308)、
  [PR #309](https://github.com/ait-app/ait/pull/309) 与
  [OpenCode 验证报告](../providers/opencode-2.0.25-compatibility-2026-10-13.md)。
- **问答和自动 Bot。** 问答响应超过五秒时恢复操作，允许保留答案重试。
  自动 Bot 执行中收到交互权限请求时立即拒绝并解释原因，不停留在无人处理的审批状态；
  客户端移除 Bot 交互权限审批界面，实验性 Bot 开关继续默认关闭。
  见 [PR #307](https://github.com/ait-app/ait/pull/307)、
  [PR #314](https://github.com/ait-app/ait/pull/314)。
- **Relay 与移动端连接。** 复用 Host 侧 TLS 会话，启用 TCP_NODELAY，改进连接复用、失败后的
  恢复与退避。原生移动端进入后台后保留账户 Relay 连接两分钟，回到前台恢复生命周期管理。
  见 [PR #313](https://github.com/ait-app/ait/pull/313)、
  [PR #312](https://github.com/ait-app/ait/pull/312) 与
  [Relay 验证范围](../clients/relay-connect-latency.md)。
- **终端、Workspace 与恢复。** PTY 在客户端订阅前回复 DA1，避免 fish 启动等待；Workspace
  reset 清理旧 PR 关联，定时任务系统标签使用新的区分名称，移除 Worktree setup 提醒。
  阻塞文件操作移出异步 reactor，恢复路径不再强制 panic。
  见 [PR #305](https://github.com/ait-app/ait/pull/305)、
  [PR #318](https://github.com/ait-app/ait/pull/318)、
  [PR #315](https://github.com/ait-app/ait/pull/315)、
  [PR #311](https://github.com/ait-app/ait/pull/311)。

## 安装与 review 范围

预期桌面附件为 Linux x86_64 的 AppImage／tar.gz、macOS Apple Silicon 的 DMG／ZIP，
更新元数据、blockmap、`BUILD-INFO.json` 和 `SHA256SUMS`，共 10 个文件。
macOS 要求 13 或更新版本。Draft 只供有权限的 reviewer 查看，不进入正式自动更新。

已有 Agent 的大图片接纳修复需要更新目标 daemon，旧 Host 不会仅因客户端更新而得到新预算。
图片总量和队列仍有上限；这不是无限容量支持。OpenCode 2.x 最低为 2.0.25，1.x 支持继续保留。
异步插件晚注册模型的原生限制仍存在，不能将本次兼容性修复理解为所有插件模型发现已解决。

移动端后台保持和原生图片编码属于共享源码的改进；本次桌面标签不触发 Android APK、
Google Play、iOS TestFlight 或 AUR 发布，移动端原生改动需要其独立构建／发布流程交付。
准备基线中尚未合并的 PR（包括 #317、#306、#292、#240）不纳入本次更新说明。

## 发布准备与门禁

Rust workspace 与 15 个本地 Cargo 包、根 npm 包和 6 个 workspace、两份锁文件及根
`PKGBUILD` 同步为 `0.1.2`；第三方依赖保持准备基线的锁定记录。
发布准备只修改版本与文档，不修改 Rust 源码、CI 工作流或 GitHub 审批环境。

按[发布指南](https://github.com/ait-app/ait/blob/fe5d25fe6d0b320d77e9fe523be26c7f3ac4a5c5/docs/operations/releasing.md#draft-后必须停下等待人工-review)完成锁定依赖、
版本一致性、发布脚本、桌面主进程、类型检查、格式和文档验证。PR／main 适用 CI 通过后
创建不可变标签，发布构建完成两平台安装包、macOS 签名／公证及成品 daemon 生命周期门禁。

本地验证通过：

- `npm ci --no-audit --no-fund` 安装 2288 个锁定包；版本校验与 6 个本地 workspace 链接验证。
- `npm run test:release` 53 项与 `npm run test:mobile-release` 41 项全部通过。
- 桌面主进程构建、桌面和移动端类型检查通过。
- Oxfmt 检查 3338 个文件；变更文件 lint 检查无需执行 JavaScript／TypeScript lint。
- 文档检查的 1 项测试与 358 份 Markdown 本地链接验证通过。
- `cargo metadata --locked --offline --no-deps` 确认 15 个包版本一致；`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check` 通过。对照基线的结构化锁文件核验确认第三方记录未变。

交付前核验 Draft 状态、10 个附件的上传摘要、更新元数据和 `BUILD-INFO.json` 中的源码、
工作流提交与运行链接，确认公开 Latest 仍为 `v0.1.1`，然后停止等待用户 review。
本次不执行将 Draft 转为正式版的命令；用户随后明确确认发布 `0.1.2` 才继续。

## Test coverage

**Not measured。** 用户未请求覆盖率。本次只准备版本和文档，没有修改 `bins/` 或
`crates/` 的 Rust 源码，按仓库要求跳过本地 Rust 测试与覆盖率；既有 CI 门禁照常运行。
上面链接的功能验证属于各自修复任务，不能作为本次发布源码的覆盖率。
若后续请求测量，选择相关 crate 和定向测试并提供可共享报告；工作区覆盖率另需明确请求。
