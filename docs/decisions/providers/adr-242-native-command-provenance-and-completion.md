---
id: ADR-242
status: proposed
date: 2026-11-11
paths:
  - crates/provider
  - crates/api
  - packages/protocol
  - packages/client
  - apps/mobile
---

# ADR-242：原生命令来源、补全与输入交互（PR612 task06）

- 范围：OMP 命令目录/补全/输入交互；不改变其他 provider 的既有命令行为。
- 状态说明：本 ADR 随 Draft PR 提交，`proposed` 表示等待架构评审；Draft 不是评审批准。
  不导入 donor 的 accepted 状态，历史 ADR 编号与内容保持不变。

## 背景

主线已有 `AgentClient::commands` 发现端口与 `agent.commands.list.request` 草稿查询，
但 OMP 目录投影丢弃原生 `source`/`aliases`，菜单无法区分内置与插件/技能/模板/项目命令；
内置管理命令与终端专属命令可能被手输后转发给模型；没有原生编辑器补全；
参数提示若直接写入草稿会污染提交、复制与历史。donor PR612 的对应实现与其
预设/角色/BTW/Vibe 基础设施耦合，本任务只选择性重写命令来源、补全与输入交互部分。

## 决策

1. 命令目录按 host/连接/provider/cwd/原生会话隔离。OMP 会话创建/恢复/配置重开时读取
   `get_available_commands`；`available_commands_update` 更新原始所有权并经既有 Agent stream
   发布 `commands_changed`，客户端取消并失效目录与补全查询。旧上下文响应不进入新查询。
   已注册 Agent 的目录请求经其会话 lane 串行（`supports_session_commands`）；
   草稿继续用一次性 `--no-session` 探测，不创建 Agent、不提交输入。
2. 内置过滤只作用于目录中**显式** `source == "builtin"` 的有效 owner 条目（含别名匹配）。
   同名 plugin/skill/template/project 条目保留原生所有权与调度，不被黑名单误伤；
   goal/btw/vibe 等宿主特性名同样按实际来源路由，06 不实现任何同名拦截。
   来源缺失或无法识别（旧运行时）显式降级为原生调度，绝不按名称猜测内置；
   目录中不存在的名称也不被假定为内置。
   被屏蔽内置的手输文本得到简短可读拒绝（普通 assistant 文本时间线项）。
   `AgentSession::input_refusal` 提供 typed、input-correlated 事实；manager 在启动输入或
   中断当前 turn 之前验证 message id，只追加该注记；不扫描/丢弃 provider 事件，不以显示键
   或文本判定拒绝。`InputRefused` 经既有 lane/receipt 结算 `Rejected`，首次与 replay 均返回
   `invalid_message`；不导航、不转发模型、不关闭 writer，Idle 保持 Idle，活动 turn 不受影响。
   `start` 与 `steer_input` 使用同一来源准入；其他 provider 与普通失败语义不变。
   本任务不新增内置查询执行处理器，也不引入通用 native_action 网关或
   Pending/Admission 异步回执基础设施；原生命令终态确认沿用既有
   prompt admission/prompt_result/session_settled 生命周期。
   屏蔽名单依据冻结命令处置与主线事实：GUI 已承接的管理/导航内置、终端专属内置、
   未交付工作流（btw/vibe/goal/plan/loop/queue/tan/omfg/cleanse/extensions/status/stats/trace/seance），
   以及可能在 task25 原子核对之外改变会话身份/历史的内置（handoff/new/clear/delete/resume/
   branch/rewind/fork/tree；官方 OMP 18.17.0 实测不提供这些内置，名单仅防御其他运行时）。
   主线实际支持的其余原生 handler（/compact、/usage、/session、/memory、/tools、/context、
   /jobs、/dirs、/changelog、/security、/force、/ratchet 等）保留既有执行，不在 06 禁用。
   未交付名称的拒绝文本明确说明“尚不可用”，不暗示存在替代 Ait 控件。
3. 目录投影携带真实 `source`、`aliases` 与元数据 `argumentHint`（原生 `input.hint`）。
   保留完整所有权条目与原始 aliases，菜单单独使用 `menuVisible`/`menuAliases`；
   live 与 detached 投影一致，隐藏某个 builtin 不代表其名称可由客户端占用。
   无元数据的命令不编造提示。UI 参数提示是装饰性覆盖层：不进入草稿、提交、复制、
   历史或 IME 组合文本；选中命令只填入 `/name ` 并聚焦输入框。
4. 原生补全是独立只读能力：ready 帧协商 `capabilities.commandCompletion.version == 1`，
   会话内 `complete` 请求；未声明时显式降级为 `supported:false`，客户端回退目录过滤。
   已支持原生补全但返回零项时隐藏菜单，不回退目录，也不虚构运行时能力。
   实测官方 OMP 18.17.0 的 ready 帧不声明该能力（capabilities 为空），当前真实运行时
   走降级路径；协商路径由 fixture 验证，不宣称官方运行时已支持补全。
   `CompletionInput/CompletionItem` 使用 UTF-16 偏移并在 adapter 边界一次性校验
   （范围、caret、去重、尺寸预算）；客户端按原生编辑精确应用，不重排候选。
   补全候选同样经过内置过滤：被屏蔽内置的编辑结果不出现。
5. 客户端 slash 快捷命令的让位依据**实际原生目录条目**（名称/别名命中即让位），
   适用于所有 provider，不按 provider id 列表，也不解析原生 JSON 语义；
   未加载/失效/失败的目录不授权客户端操作；成功空目录才证明无 claimant。submit、queue、
   建议构造与选择时复用同一 resolver，别名身份与后端一致地去尾冒号，原生插入文本保持原样。
   组合框键盘导航让位于文本编辑与 IME（`isComposing`/keyCode 229 与编辑器目标判定），
   多行方向键优先移动光标。
6. 以 `/` 开头的提交保留原始首尾空白（provider 可能据此判定命令），其余文本仍 trim。

## 接口变化与迁移

| 边界              | 旧接口/调用链                                                            | 新接口/调用链                                                                                                                | 调用方迁移与兼容性                                                                                                                                    |
| ----------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| 命令目录 wire     | `agent.commands.list.response` 条目仅 name/description/argumentHint/kind | 新增可选 `source`、`aliases`、`menuVisible`、`menuAliases`                                                                   | 所有权与展示分离；旧 host 缺省 menu 字段按可见处理；本次所有 UI 调用方统一使用当前目录所有权。                                                        |
| 目录读取路由      | 所有目录请求走共享发现通道                                               | OMP 已注册 Agent 的目录请求加入其会话 lane（`AgentClient::supports_session_commands` + `AgentSession::refresh_commands`）    | 内部 Rust 端口新增默认实现；其他 provider 不变。客户端无感知。                                                                                        |
| 补全 wire（新增） | 无                                                                       | `agent.commands.complete.request/response`（text/cursor UTF-16；supported/items）                                            | 新能力按方法名协商；旧 host 无此方法 → SDK 捕获 not_implemented 降级为目录回退；旧客户端不发送。capability 标志 `agentCommandCompletion` 为可选增量。 |
| Rust 端口         | `AgentSession` 无拒绝预检/补全/刷新；`AgentClient` 无补全                | `AgentSession::input_refusal/complete_commands/refresh_commands`、`AgentClient::complete_commands/supports_session_commands` | 默认无拒绝/unsupported/None；OMP 显式实现；不新增通用准入框架。                                                                                       |
| 内置手输          | 文本原样转发原生 prompt（可能进入模型）                                  | builtin owner 命中屏蔽名单 → 本地拒绝文本 + Rejected receipt                                                                 | 用户可见行为变化即任务目标；插件同名命令不受影响。                                                                                                    |
| UI 输入           | 无参数提示；组合框方向键可劫持编辑器                                     | 装饰性 InlineCommandHint；`shouldHandleComboboxKey` IME/编辑器让位；按目录条目让位原生所有权                                 | 仅 apps/mobile 内部组件 props 增量（`inlineCommandHint` 可选）；无公共 SDK 破坏。                                                                     |
| 提交文本          | 一律 trim                                                                | `/` 开头保留原文                                                                                                             | 修复命令判定；普通消息行为不变。                                                                                                                      |
| 时间线/持久化     | assistant_message 等既有 item                                            | 不变                                                                                                                         | 拒绝回执是普通 assistant 文本；不新增 durable item 类型，旧封闭联合不受影响。                                                                         |
| 目录更新通知      | 无                                                                       | `AgentTurnEvent::CommandsChanged` → Agent stream `commands_changed`（provider）                                              | 既有 stream 顺序交付；当前客户端取消并失效该 Agent 的目录与补全查询，不引入独立订阅通道。                                                             |

删除项：无。本任务不删除任何既有公共接口；donor 的 `native_action`、预设 RPC、
providerCommand 时间线元数据等均未导入。

## 后果与验证

菜单/补全显示真实来源；同名插件不被误拦截；被屏蔽内置不进入模型；提示不污染草稿。
验证归属：provider 定向测试（策略单元、fixture 会话行为、可选安装 OMP 实测）、
protocol/client/mobile 定向类型与测试、真实 OMP 运行时与浏览器 UI 冒烟。
macOS/Windows、原生键盘与签名包不在本任务验收范围。task01/task31 按用户决定跳过；
保留已验证 task01 依赖代码；缺少 macOS 证据不是 task06 downstream blocker，也不声称已完成。
当前说明见 [Provider 架构](../../architecture/provider.md) 与 [OMP 操作手册](../../operations/oh-my-pi.md)。
