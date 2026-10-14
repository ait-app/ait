# Ait 0.1.3 Draft 发布说明

日期：2026-10-14（Asia/Shanghai）。发布准备基于 main `065e06917ba655a9a5ca9f0019098004129f2a2e`。
每日发布检查确认 `v0.1.2` 后已有新增功能，准备补丁版本 `0.1.3`。
本次交付停在 Draft Release 等待用户 review，公开 Latest 继续为 `v0.1.2`。
最终安装包来源以不可变标签 `v0.1.3` 和附件 `BUILD-INFO.json` 为准。

## 更新内容

- **daemon MCP 编排。** 外部 MCP 客户端可以连接 `/mcp/agents`，通过 40 个工具管理 Agent、
  Workspace、终端、脚本、定时任务、心跳与权限。入口使用现有 daemon 令牌认证，复用既有
  输入接纳、幂等创建和权限规则；`mcp.enabled` 默认开启，配置修改即时生效。
  见 [PR #325](https://github.com/ait-app/ait/pull/325)、
  [使用说明](../../operations/mcp.md)与[实现验证](../daemon/mcp-implementation-2026-10-14.md)。
- **DeepSeek Harness 插件情景模式。** 从所选原生 profile 的 `agentPresets/list` 动态发现
  自定义／插件情景模式，使用原生权限目录独立选择权限，不再将权限误当作情景模式。
  既有和导入会话保留原组合，旧权限 ID 继续兼容。
  见 [PR #229](https://github.com/ait-app/ait/pull/229)、
  [使用说明](../../operations/deepseek-harness.md)与[验证范围](../providers/dsh-plugin-presets.md)。
- **配额与余额。** 恢复 Antigravity 原生模型组五小时／每周配额窗口，展示剩余比例与重置时间。
  DeepSeek Harness 支持官方 DeepSeek API key 的只读余额查询，保留原币种、总额、充值余额、
  未过期赠送余额与可用状态；无 API key 时继续查询原生登录账户余额。
  见 [PR #321](https://github.com/ait-app/ait/pull/321)、
  [Antigravity 验证](../providers/antigravity-account-quota-2026-10-14.md)与
  [DeepSeek 余额验证](../providers/dsh-deepseek-balance-2026-10-14.md)。
- **Workspace 与界面。** 缓存保存初始分支，重新打开 Workspace 后仍可 reset；旧缓存会重新
  拉取 Workspace 投影。新建标签菜单移除 Edit Profiles；空工具详情不再显示展开入口，
  有实际详情的工具继续可展开。更新 fork 提示的 Ait 品牌并清理过时帮助链接。
  见 [PR #324](https://github.com/ait-app/ait/pull/324)、
  [PR #322](https://github.com/ait-app/ait/pull/322)、
  [PR #320](https://github.com/ait-app/ait/pull/320)。

## 安装与 review 范围

预期桌面附件为 Linux x86_64 的 AppImage／tar.gz、macOS Apple Silicon 的 DMG／ZIP，
更新元数据、blockmap、`BUILD-INFO.json` 和 `SHA256SUMS`，共 10 个文件。
macOS 要求 13 或更新版本；正式版本构建必须通过 Developer ID 签名、公证和成品门禁。
Nightly 仍使用 ad-hoc 签名且未公证。Draft 只供有权限的 reviewer 查看，不进入正式自动更新。

MCP 客户端需要显式配置连接与 Bearer 令牌；本次未实现 Provider 自动注入、Browser 或 Voice
会话工具。Heartbeat 会持久化，任务完成后应显式删除；Schedule 工具只操作 new-agent 任务。
更新远程 Host 的 daemon 后才能使用新的 MCP 入口。

DeepSeek Harness 默认使用支持 WebHost 的 `web` profile，插件须在所选原生 profile 中可用；
不支持从 CLI 启动 Electron 专属 desktop profile，也不提供插件专用 Web 界面。
改变既有会话的情景模式需要新建会话，显式 ACP 兼容模式不支持这些原生能力。
DeepSeek API key 优先于登录账户，余额不会合并；自定义 API 地址、未知或可执行凭据后端
以及不具备所需能力的旧 CLI 会明确显示不可用，不会伪装成零余额。

本次桌面标签不触发 Android APK、Google Play、iOS TestFlight 或 AUR 发布。
准备基线中尚未合并的 PR（包括 #317、#306、#240、#230、#174）不纳入本次更新说明。

## 发布准备与门禁

Rust workspace 与 15 个本地 Cargo 包、根 npm 包和 6 个 workspace、两份锁文件及根
`PKGBUILD` 同步为 `0.1.3`；第三方依赖保持准备基线的锁定记录。
发布准备只修改版本与文档，不修改 Rust 源码、CI 工作流或 GitHub 审批环境。

按[发布指南](../../operations/releasing.md#draft-后必须停下等待人工-review)完成锁定依赖、
版本一致性、发布脚本、桌面主进程、类型检查、格式和文档验证。PR／main 适用 CI 通过后
创建不可变标签，发布构建完成两平台安装包、macOS 签名／公证及成品 daemon 生命周期门禁。

本地验证通过：

- `npm ci --no-audit --no-fund` 安装 2288 个锁定包；版本校验与 6 个本地 workspace 链接验证。
- `npm run test:release` 53 项与 `npm run test:mobile-release` 41 项全部通过。
- 桌面主进程构建、桌面和移动端类型检查通过。
- Oxfmt 检查 3348 个文件；变更文件 lint 检查无需执行 JavaScript／TypeScript lint。
- 文档检查的 1 项测试与 366 份 Markdown 本地链接验证通过。
- `cargo metadata --locked --offline --no-deps` 确认 15 个包版本一致；`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check` 通过。结构化锁文件对比确认第三方记录未变。

初次移动端发布测试与依赖安装同时启动，因缺少尚未安装的依赖失败；安装完成后重跑 41 项
全部通过。此失败没有引入源码或 CI 修改。

交付前核验 Draft 状态、10 个附件的上传摘要、更新元数据和 `BUILD-INFO.json` 中的源码、
工作流提交与运行链接，确认公开 Latest 仍为 `v0.1.2`，然后停止等待用户 review。
用户随后明确确认发布 `0.1.3` 才继续。

## Test coverage

**Not measured。** 用户未请求覆盖率。本次只准备版本和文档，没有修改 `bins/` 或
`crates/` 的 Rust 源码，按仓库要求跳过本地 Rust 测试与覆盖率；既有 CI 门禁照常运行。
上面链接的功能验证属于各自实现任务，不能作为本次发布源码的覆盖率。
若后续请求测量，选择 MCP、Provider 或 Workspace 缓存的相关定向测试并提供可共享报告；
工作区覆盖率另需明确请求。
