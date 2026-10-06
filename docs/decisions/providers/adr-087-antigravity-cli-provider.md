# ADR-087：Antigravity CLI 原生 Provider

- 状态：Accepted，2026-10-06。
- 范围：`crates/provider`、`bins/daemon`、共享协议与客户端 Provider 显示。
- 上位约束：ADR-027、ADR-031、ADR-039、ADR-072。

## 背景

Google 的 Antigravity CLI 使用 `agy` 命令。官方脚本与 Homebrew cask 安装同一原生 CLI，
安装目录不同；Electron 启动的 daemon 也可能没有终端的 PATH。
本机 Homebrew CLI 1.3.0 的帮助、模型列表和流式输出已与官方协议对照。

参考：[官方安装与认证](https://antigravity.google/docs/cli/install/)、
[Headless 协议](https://antigravity.google/docs/cli/headless)、
[Homebrew cask](https://formulae.brew.sh/cask/antigravity-cli)。

## 决策

1. 增加 `local/antigravity::AntigravityClient`，实现现有 `AgentClient` / `AgentSession` ports。
   稳定 Provider ID 为 `antigravity`，可执行命令为 `agy`。
   CLI 协议和进程管理只在 adapter 内；注册、输入接纳、队列与时间线持久化仍由 AgentManager 管理。
2. `AIT_SERVER_ANTIGRAVITY_BIN` 可以指定命令或路径，显式设置失效时不切换其他安装。
   默认依次搜索 PATH、官方用户安装目录、`HOMEBREW_PREFIX/bin` 和常见 Homebrew 路径。
   不将 Antigravity IDE 的启动命令视为 CLI；不安装软件、不复制凭据、不修改原生 settings。
3. 每个 live Session 拥有一个 `--input-format stream-json --output-format stream-json` 进程。
   等待 `init` 后登记 conversation ID；每轮只向 stdin 写入一个 `user` 消息。
   写入失败或超时不重发。模型/模式/effort 变更关闭旧 writer，再以 `--conversation` 连接同一会话。
   本机 CLI 自动更新在 daemon-owned 子进程中关闭。
4. 模型由有界 `agy models` 查询动态发现。保留完整 slug 与 label，不猜测默认模型或各模型支持的 effort。
   CLI 的 `--effort` 参数可通过配置指定；native 模型 slug 本身也提供不同 effort 变体。
   模式为 Local Permissions、Accept Edits、Plan、Full Access；只有显式 Full Access 添加
   `--dangerously-skip-permissions`。Local Permissions 保留本机 AGY 规则。
5. `step_update` 映射文本增量、完整工具快照和完成条目，使用 `native:` 前缀及 conversation/turn/step 标识，
   避免重连后原生 step index 重用覆盖旧时间线。
   `result` 的 token usage 是累计快照，直接替换而不相加；不推算额度、费用或上下文窗口。
   失败、等待与未知 terminal status 不能当作完成。跨会话事件、超限输出和成功时未完成的步骤均失败。
   工具输出显示预览有界；原生完整输出仍归 AGY。
6. persistence handle 只保存 conversation ID 与 cwd。认证和原生会话由 AGY 保存。
   官方协议没有完整只读 transcript/list API，adapter 不宣称 history replay；daemon 保留已存时间线。
   Unix 中断发送 SIGINT，只有收到原生 `CANCELED` / `INTERRUPTED` 才确认成功。
   之后关闭旧进程，下轮按 conversation ID 重连；Windows 不宣称支持原生取消。
   关闭和 Drop 回收 writer、reader 与 Unix 进程组。
7. 官方 stream input 不支持控制/审批消息和非文本块。
   保留 native permission soft-denial，不伪造 Ait 审批；拒绝图片、schema、system prompt、
   会话级 MCP 设置与未实现的 provider options。文本附件经现有有序 blocks 转换传入。
   CLI slash expansion 关闭，避免终端命令破坏流式协议。

## 后果与验证

桌面、Web 与移动端通过已有 Provider snapshot 获取 Antigravity，显示既有 AGY 图标并提供
`agy --conversation <id>` 终端恢复命令。无需改变 domain 类型或新增服务。

安装与能力限制见 [操作手册](../../operations/antigravity.md)，
测试与平台范围见 [验证报告](../../reports/providers/antigravity-cli.md)。
