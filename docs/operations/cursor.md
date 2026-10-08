# Cursor CLI

Ait 内置 `cursor` Provider，通过官方 Cursor CLI 的 `acp` 子命令接入。
桌面、Web 和移动端都使用 daemon 所在机器的 Cursor 安装与认证。

## 安装与登录

按 [Cursor 官方安装文档](https://cursor.com/docs/cli/installation)安装 CLI，
再在 daemon 所在机器运行 `cursor-agent login`（新版本也提供 `agent login`）。
安装 Cursor 编辑器本身不等于安装 CLI。

Ait 依次查找 PATH 和 `~/.local/bin` 中的 `cursor-agent` / `agent`。
桌面启动时 PATH 不完整，仍可发现用户目录安装。
指定其他安装路径时，在启动 daemon 前设置：

```sh
export AIT_SERVER_CURSOR_BIN=/absolute/path/to/cursor-agent
```

显式路径不可执行时显示 unavailable，不切换其他安装。Ait 不安装 CLI、
不复制凭据，也不直接编辑 Cursor 配置文件。现有 `CURSOR_API_KEY` / `CURSOR_AUTH_TOKEN` 环境认证
由 CLI 处理；不要将 token 写入仓库或 Provider 配置。

## 使用与能力

在 Agent 创建入口选择 Cursor。模型目录使用原生只读 `cursor/list_available_models` 扩展，
默认模式取自 ACP 会话返回值；按模型分别展示思考选项与 Fast 控件。
CLI 必须支持该模型扩展；旧版返回 method-not-found 时需要升级，不退回不完整的会话模型列表。
发现、草稿控件和选择校验不会调用配置 setter。用户实际选择模型、模式、思考或 Fast 后，
由 CLI 的原生 setter 应用；Cursor 自己可能保存这些偏好。
Fast 在 Ait 使用与 Codex、Claude Code 相同的 `fast_mode` 布尔配置，转换为原生 `fast` 的字符串值。
不支持 Fast 的模型不展示开关，显式启用时拒绝；已禁用的 Fast 值可以保留。
支持原生 Agent、Plan、Ask 模式；模式和工具审批彼此独立，Agent 模式仍遵循 Cursor 权限规则。
Ait 不提供伪造的免审批模式。原生工具审批、选择题和计划确认通过现有审批界面答复。

支持流式文本、推理、工具状态、ACP 计划任务、原生上下文及 prompt 返回的 token 用量。
用量快照替换旧值，不累加重复计数。原生命令列表通过异步 `available_commands_update` 接入，
命令查询最多等待十秒。图片输入仅在 CLI 协商支持时可用。
取消使用 ACP `session/cancel`，答复待处理请求为 cancelled，并以原生返回的取消结果结束当前轮次；
未结束的工具状态会收尾，保留原始输入。
恢复优先使用声明了 `loadSession` 的 `session/load`，否则仅在协商到 resume 能力时使用 `session/resume`；
保留原生 session ID 与工作目录，拒绝返回不同 session ID 的恢复响应。
恢复阶段的历史通知与后续 live turn 隔离。
终端恢复命令为 `cursor-agent --resume <session-id>`。

MCP 由 Cursor 的用户或项目 `.cursor/mcp.json` 配置；本次接入不接受 Ait 会话级 MCP 注入。
暂不提供原生会话列表/导入、只读历史重放、rewind、自定义 system prompt 或结构化辅助生成。
尚未接入 Cursor 专用 `update_todos` 的 merge 展示、子任务/生成图片扩展通知与账户额度查询；
标准 ACP plan 与图片 content 输出已支持。
Ait 自己保存的时间线可在 daemon 重启后查看；仅进行发现也会建立短暂 ACP 会话，随后关闭进程。

协议与能力依据 [Cursor ACP 文档](https://cursor.com/docs/cli/acp)。
实现边界见 [ADR-110](../decisions/providers/adr-110-cursor-acp-provider.md)，
离线与平台验证范围见 [验证报告](../reports/providers/cursor-acp.md)。
