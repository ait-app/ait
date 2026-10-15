# ADR-149：Codex 高权限档位自动批准 computer use 访问

- 状态：已接受
- 日期：2026-10-14
- 关系：修订 [ADR-122](adr-122-codex-computer-use-approval.md) 的档位范围与原生请求处理边界。

## 背景

Auto-review 原来只预批准插件的 `cua_repl.js`。Codex 0.162.1 的实际请求显示，
computer use 访问应用时还会发出 `mcpServer/elicitation/request`：例如
`Allow Computer Use to use "Ait"?`，其 `_meta` 包含
`codex_approval_kind: mcp_tool_call` 和 `connector_id: computer-use`，表单属性为空。
这层请求仍进入人工审批，Full Access 也没有使用 computer use 档位默认值。

外层工具预批准不能代替原生服务发出的访问批准。用户选择 Auto-review 或 Full Access
时，期望这两层批准均由 Ait 自动完成。

## 决策

1. Auto-review 与 Full Access 均预批准插件的 `cua_repl.js`。显式逐工具配置继续优先；
   设置为 `auto`、`prompt` 或 `writes` 时，不执行档位自动批准。
2. Provider 先执行既有请求身份、线程、轮次、大小、数量和 schema 校验，再识别
   `cua_repl` 发出的 form 请求。只有同时带有上述两个 `_meta` 标识且
   `requestedSchema.properties` 为空的 computer use 工具访问批准才自动响应。
   判断使用 Ait 当前会话配置，不使用请求携带的 `auto_review_enabled` 等状态。
3. 自动响应使用原生单次批准 `{"action":"accept","content":{},"_meta":null}`，
   不写入全局配置或持久批准。Provider 不向客户端发布该请求，也不保留待审批项。
4. 同步轮询将响应交给有界的异步写入任务。后续轮询先检查写入结果，写入失败按
   Provider 失败处理；关闭会话或丢弃 transport 时取消任务。
5. 切入或退出自动批准档位时重建原生进程并恢复同一线程，加载或清除工具配置。
   Auto-review 与 Full Access 之间切换时，工具配置相同，审批者与沙箱按下一轮配置应用。

普通问答、其他 MCP 服务与 connector、需要填写内容的 form、认证和操作系统授权
继续使用各自的原生交互。Bot 沿用 [ADR-145](../bot/adr-145-bot-permission-denial.md)
的权限策略，Auto-review 在 Bot 中降为 `auto`。

原生空表单与批准元数据格式可对照
[Codex MCP tool call 源码](https://github.com/openai/codex/blob/main/codex-rs/core/src/mcp_tool_call.rs)。
回归夹具依据用户提供的 0.162.1 实际请求，使用合成的调用、线程与轮次身份。

## 后果与验证

Auto-review 与 Full Access 下的 computer use 应用访问自动通过并继续轮次。
默认权限和只读模式保留人工批准；显式逐工具设置可要求人工处理。
自动响应保留原生请求 ID，使用现有 MCP 回复编码，不解析 JavaScript 或审批提示文案。

验证限定于 Provider 的配置、请求处理、transport、同线程档位切换与直接相关的
普通 Agent / Bot 审批回归。实际请求的夹具在修复前返回 `PermissionRequested`，
修复后收到精确的原生 accept 回复并完成轮次。未操作真实应用或浏览器进行端到端验收。

## Test coverage

未测量覆盖率：用户未请求统计。如后续请求测量，应限定上述 Provider 审批相关测试，
不扩展为完整 Workspace 测试。
