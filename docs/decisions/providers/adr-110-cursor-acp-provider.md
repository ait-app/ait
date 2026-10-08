# ADR-110：Cursor CLI ACP Provider

- 状态：Accepted。
- 日期：2026-10-08。
- 范围：`crates/provider`、共享客户端 Provider 清单与 daemon 集成测试。
- 上位约束：ADR-072、ADR-089、ADR-091、ADR-102。

## 背景

Cursor 提供正式 ACP stdio 入口和阻塞式提问、计划审批扩展。
Ait 已有 DSH ACP 兼容适配器，其有界传输、工具审批与时间线投影可供 Cursor 复用。
官方协议见 [Cursor ACP](https://cursor.com/docs/cli/acp)。
模型发现、Fast 与异步命令参考 [Paseo Cursor ACP](https://github.com/getpaseo/paseo/blob/23d7a089e228862478972895279d4caeb6c591db/packages/server/src/server/agent/providers/cursor-acp-agent.ts)，
选择校验、用量与恢复同时对照 Ait/Paseo 的 Codex 与 Claude Code 实现，具体证据见验证报告。

## 决策

1. `local/cursor::CursorClient` 实现既有 `AgentClient` / `AgentSession` ports，稳定身份为 `cursor`。
   Provider 自行装配，daemon 无需认识具体 CLI。domain 与存储接口不变。
   `AgentSession` 增加会话控件快照与配置提交前校验两个默认 hook，应用服务统一调用，
   使 Cursor 的动态 Fast/模式可在 live 会话展示与校验；其他 adapter 使用默认行为。
   这两个通用 hook 与 DSH 预设 PR #229 的接口一致，Cursor PR 不包含 DSH 原生预设实现。
2. 安装发现查找 PATH 和用户 `.local/bin` 下的 `cursor-agent` / `agent`；
   `AIT_SERVER_CURSOR_BIN` 指定唯一 launcher。认证与原生会话归 Cursor，不复制 token，
   不直接编辑本机 CLI 配置文件。ACP 宣告 `cursor_login` 时使用 `authenticate`，用户预先在 CLI 登录。
3. 每个 live 会话独占 `acp` 子进程，通过有界 JSON-RPC NDJSON 通道协作。
   通用传输、流式投影和标准工具审批归 provider 内部 `local/acp`，DSH 与 Cursor 共用，
   各自保留 launcher、配置和会话生命周期。新模块没有向外新增 crate 依赖。
4. 模型目录以只读 `cursor/list_available_models` 为准，每个模型独立拥有思考与 Fast 参数，
   保留原生 ACP ID，声明 `_meta.parameterizedModelPicker`。要求 CLI 支持该扩展，
   缺失或格式错误均明确失败。模式以会话状态为准，兼容 configOptions 与旧 models/modes 字段。
   配置响应与异步通知保序合并，保留未重复返回的模型/模式 selector。
   发现和草稿校验不调用 setter；实际配置通过 `session/set_config_option`，
   旧字段通过 `session/set_model` / `session/set_mode`，CLI 可以持久化用户选择的偏好。
   Fast 复用 Ait 的 `fast_mode` 布尔特性，映射原生 `fast` 字符串。
   不添加全权限模式、不猜测模型名单。图片输入按 prompt capability 接纳。
   原生命令从异步 `available_commands_update` 获取，单次命令查询等待最多十秒。
5. 标准 `session/request_permission` 映射工具审批；`cursor/ask_question` 映射结构化问题，
   将用户选择的 label 精确转换回 native option ID；`cursor/create_plan` 映射计划审批。
   只有明确有效的用户答复才发送授权 outcome。主动取消为待处理 callback 返回 cancelled，
   通知不产生自动授权。标准 ACP plan 投影为任务列表；prompt token 用量与原生上下文合并为替换快照。
6. persistence handle 仅含 provider、native session ID 与 cwd。恢复优先使用协商后的 `session/load`，
   仅在无 load 且宣告 resume 能力时使用 `session/resume`；核对返回 ID，初始化通知不进入新轮次。
   不宣称只读历史重放或原生列表导入；daemon 保留自身时间线。
   取消使用 `session/cancel`，终止时收尾未完成工具快照；停止与 Drop 回收 reader、child 和 Unix 进程组。
7. Cursor 本地 MCP 配置继续由 CLI 管理，会话级 MCP、system prompt、tool policy 和未知高级配置
   明确拒绝。不提供结构化辅助生成或 rewind。

## 后果与验证

客户端复用既有 Cursor 图标，选择列表增加 Cursor，默认模式取原生返回值；
终端入口提供 native session resume 命令。
共享 ACP 提取需要同时验证 DSH 原生及兼容模式，避免影响原有会话投影。
本机无 Cursor CLI，离线夹具验证协议和真实 daemon 接入，在线推理留待安装并登录后的环境验证。

参见 [操作手册](../../operations/cursor.md)与[验证报告](../../reports/providers/cursor-acp.md)。
