# 命令来源与补全接口台账

本页登记原生命令来源（provenance）、参数提示与原生补全的**接口边界**、**当前检查状态**和
**尚未闭合的集成点**。决策过程与迁移表见 [ADR-242](../decisions/providers/adr-242-native-command-provenance-and-completion.md)
（`proposed`，Draft PR 不构成架构评审批准）；生效规则并入 [Provider 规格](provider.md)；
OMP 侧可见行为见 [Oh My Pi 操作手册](../operations/oh-my-pi.md#命令来源与补全)。
本页只记录事实和状态，验证证据随 PR 描述提交。

## 1. 接口面

| 面                | 所有者                                                                           | 输入/输出                                                                                                                           | 降级与失败语义                                                                                                                                         |
| ----------------- | -------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 原生命令目录投影  | `crates/provider/src/local/omp/config.rs`（`commands`）                          | 原生 `get_available_commands` → `{name, description, argumentHint, kind, source?, aliases?}`                                        | 目录非数组或超过 4096 条返回 failed；`source`/`aliases` 仅在原生显式提供时投影，不补造；空字符串与含控制字符的名称被拒绝                               |
| 来源过滤策略      | `crates/provider/src/local/omp/commands.rs`                                      | `TUI_ONLY` / `MANAGED` / `UNDELIVERED`；`split`、`blocked`、`catalog`、`rejection`、`rejection_note`                                | 仅全部 claimant 显式为 builtin 时拒绝；未知/扩展来源保留原生调度。完整条目/原始别名保留，`menuVisible`/`menuAliases` 独立控制菜单；live/probe 共用投影 |
| 会话目录所有权    | `crates/provider/src/local/omp/session.rs`（`native_commands`）                  | 创建/恢复/配置重开时读取目录；`available_commands_update` 刷新原始目录并发出 `AgentTurnEvent::CommandsChanged`                      | 流式 `commands_changed` 通知使前端取消并失效目录/补全查询；读取失败不伪装为空目录                                                                      |
| 目录读取路由      | `crates/provider/src/service/agent_execution/routing.rs`（`uses_command_owner`） | 已注册 Agent 的 `Commands`/`CompleteCommands` 在 `supports_session_commands` 为真时加入其会话 lane                                  | 其他 provider 或草稿身份保持既有非 lane 发现；未知 Agent 走草稿或返回 `AgentNotFound`                                                                  |
| 服务协调          | `crates/provider/src/service/agent_manager/controls.rs`                          | `refresh_commands`（仅 live 会话）、`uses_session_commands`、`complete_commands`（live owner 或一次性隔离探测）                     | 无 live 会话时回落到 `AgentClient::complete_commands` 的 `--no-session` 探测，不注册 Agent；`Unavailable` → `UnsupportedCapability`                    |
| Rust 端口         | `crates/provider/src/ports/agent_session.rs`                                     | `AgentSession::{input_refusal, complete_commands, refresh_commands}`、`AgentClient::{complete_commands, supports_session_commands}` | 默认无拒绝 / unsupported / `Ok(None)` / `false`；其他 adapter 保持原有行为                                                                             |
| 拒绝文本          | `crates/provider/src/local/omp/commands.rs`（`rejection`/`rejection_note`）      | 普通 `assistant_message` 时间线项；显示键不参与准入或 receipt 判定                                                                  | `InputRefusal` 携带输入标识及显示项；`start` 与 `steer_input` 使用同一来源策略                                                                         |
| 拒绝结算          | `crates/provider/src/service/agent_manager.rs` + `.../delivery.rs`               | 原生输入/中断前检查 typed `InputRefusal`，校验当前 message id；只写其注记；`InputRefused` 使既有 receipt 结算 `Rejected`            | 不扫描、不丢弃其他事件；不启动输入、不关闭 writer、不打断既有 turn；Idle 保持 Idle；首次与 replay 均返回 `invalid_message`                             |
| 目录 wire         | `packages/protocol/src/messages.ts`                                              | `agent.commands.list.request/response`；条目可选 `source`、`aliases`、`menuVisible`、`menuAliases`                                  | 来源/别名承担所有权，menu 字段承担可展示性；不以隐藏菜单证明无原生 owner                                                                               |
| 补全 wire         | 同上                                                                             | `agent.commands.complete.request/response`：`text`+`cursor`（UTF-16）→ `supported`+`items`                                          | 旧 host 无方法 → SDK 以 `not_implemented` 拒绝，客户端显式降级为目录回退；旧客户端从不发送                                                             |
| 能力协商          | `packages/client/src/protocol/messages.ts`（`serverInfo`）                       | `agentCommandCompletion = has("agent.commands.complete.request")`                                                                   | 客户端侧门控；`packages/protocol` 的 `features.agentCommandCompletion` 为可选字段                                                                      |
| 原生补全适配      | `crates/provider/src/local/omp/completion.rs` + `transport.rs`                   | ready 帧 `capabilities.commandCompletion.version == 1` 才发 `complete`；否则不发 I/O 直接 `supported:false`                         | 原生输出经 `CompletionInput/Result` 一次校验；被屏蔽内置的编辑结果按编辑后文本重算 `blocked` 后剔除                                                    |
| 补全校验          | `crates/provider/src/protocol/commands.rs`                                       | 文本 ≤ 65536 字节、无 NUL、caret 不切开代理对；items ≤ 256、序列化 ≤ 896 KiB、id 唯一、替换范围合法                                 | 任一不满足返回 failed/rejected，不把畸形 payload 暴露给上层                                                                                            |
| SDK 方法          | `packages/client/src/daemon-client.ts`（`completeCommands`）                     | `ListCommandsOptions & {text, cursor}`，超时 30s                                                                                    | 草稿或未激活会话可能先启动只读运行时；方法缺失由上层捕获 `not_implemented`                                                                             |
| 客户端补全查询    | `apps/mobile/src/hooks/use-agent-command-completions-query.ts`                   | 键含 server/agent/连接身份/context/draft/text/cursor/revision；120ms 去抖；能力门控不依赖目录非空                                   | 查询取消后忽略响应；刷新期间不展示旧结果；`not_implemented` 显式回退，其他错误保持错误                                                                 |
| 客户端目录上下文  | `apps/mobile/src/hooks/agent-commands-query.ts` + `use-agent-command-context.ts` | 目录/补全身份均包含连接与 `{provider, cwd, nativeSessionId}`                                                                        | 未加载、正在刷新、已失效、失败均不授予客户端所有权；成功空目录与未加载区分；提交/队列/选择按当前缓存重验                                               |
| 参数提示          | `apps/mobile/src/composer/command-argument-hint.ts` + `inline-command-hint.tsx`  | 仅 slash 名称后的空参数区显示原生 `argumentHint`；不可点击、无障碍隐藏；整个 IME 组合期间隐藏                                       | 无元数据或已有参数不显示；不写入草稿、提交、复制或历史                                                                                                 |
| 补全应用          | `apps/mobile/src/utils/native-command-completion.ts`                             | 按原生 `replaceRange`/`insertText`/`cursor` 精确替换；拒绝切代理对或越界                                                            | 返回 `null` 时不改动编辑状态                                                                                                                           |
| 自动补全编排      | `apps/mobile/src/hooks/use-agent-autocomplete.ts`                                | 保留原生候选顺序；选中二次校验当前目录所有权、候选、`inputText`/`inputCursor`/无选区                                                | `supported:false` 回退目录；支持原生补全但返回零项时隐藏菜单，**不回退目录**                                                                           |
| 客户端 slash 让位 | `apps/mobile/src/client-slash-commands/index.ts`                                 | `resolveClientSlashCommand` 统一 submit/queue/建议/选择；名称或去尾冒号的别名命中任意来源即让位                                     | 不修改原生插入文本；未知来源仍占有名称；目录未加载不执行客户端快捷命令，成功加载且无 claimant 才可执行                                                 |
| 键盘与输入法      | `apps/mobile/src/components/ui/combobox-keyboard.ts`                             | 复用 `isImeComposingKeyboardEvent`；方向键、Enter、Escape 在 IME 期间全部让位                                                       | 菜单搜索框也遵守 IME；非组合期间继续使用其方向键行为                                                                                                   |
| 提交文本          | `apps/mobile/src/composer/submit.ts` + `actions.ts`                              | `/` 开头保留原始首尾空白，其余 trim                                                                                                 | 队列与失败恢复使用同一 text；普通消息行为不变                                                                                                          |

## 2. 当前检查状态

精确命令、当前集成结果与历史 writer 结果登记在 [task06 记录](../plans/pr612-split/task06.json)。
当前集成已执行移动端 8 文件 / 56 个定向测试、protocol 与 client 定向测试、
移动端/协议/客户端 typecheck、31 个改动 UI/配置文件的 `check:ui` 与 oxlint（0 warnings / 0 errors）
以及 oxfmt 全量通过，还有 typed refusal 与 provider 目录/补全的定向 Rust 测试。

官方 OMP 18.17.0 的真实 `rpc-ui --no-session` 进程已接受合法 `/new` extension：
目录实际 `source=extension`，`/new exact-argument` 输出 `NEW-PLUGIN-NATIVE-OK:exact-argument`，
原生会话结算且未启动模型；这是原生证据，**不是 Ait 浏览器路由验收**。
真实浏览器 spec 已在隔离 HOME/XDG/TMPDIR 与回环 fixture 模型（无推理）下通过一次：
菜单列出 `/compact` 且不列 `/model`，`/new` extension 经键盘接受后原生输出
`NEW-PLUGIN-NATIVE-OK`、路由不变，`/record` 与 `/goal` 插件原生分发，
builtin `/model` 得到可读拒绝，`/compact` 参数提示来自原生元数据且草稿不变。
已记录的既有缺陷（非本任务引入）：基线 22efa58f 与本分支一致地无法用指针点击应用补全行，
键盘接受路径正常；该项作为 blocker 登记，不计为通过。

## 3. 开放集成点与范围

1. **目录投影已统一**：live 与 detached 共用 `commands::catalog`，保留完整 ownership；
   菜单排除明确被拒绝的 builtin 名称/别名，隐藏条目不让出客户端快捷命令所有权。
2. **能力协商 vs 真实运行时**：官方 OMP 18.17.0 未声明 `commandCompletion`，
   真实运行时走目录回退；原生补全能力的行为由 fixture 覆盖，不能宣称官方已支持。
   官方 18.17.0 的 builtin registry 优先于同名 extension：注册 `/model` 不会产生 extension claimant；
   实际合法 Ait-name collision 使用 `/new`，不伪造双 claimant 的真实运行时证据。
   官方扩展提示为通用 `arguments`，忽略扩展声明的具体 hint；具体参数提示使用 `/compact` 原生元数据验收。
3. **已关闭的源码缺口**：不再有 `hasNativeCatalog` mount latch、显示键前缀结算、
   扫描并丢弃事件、未加载目录执行客户端 reset，或 `available_commands_update` 不通知前端的路径。
   实际检查边界见上节，不能据源码宣称浏览器切换场景已经验收。
4. **明确排除**：task01/task31 按用户决定跳过；保留已验证 task01 依赖代码。
   macOS/Windows、原生键盘与签名包不在 task06 验收范围，不是 downstream blocker，也不声称完成。
5. **未修复的既有缺陷**：web 组合框补全行的指针点击在基线 22efa58f 与本分支上都不更新草稿；
   键盘接受（Tab/Enter）正常。它不是本任务的回归，也不在本任务范围内修复，
   因此不计为已通过，浏览器验收使用键盘路径。
6. 本页已登记于 [架构索引](README.md)；ADR-242 保持 `proposed`。
