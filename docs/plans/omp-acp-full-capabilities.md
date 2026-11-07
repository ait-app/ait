# OMP 完整能力：ACP 优先的接入方案

日期：2026-10-30。状态：设计与差异清单，**不是完整能力已交付的声明**。
本次已落地的基础修复见[验证报告](../reports/providers/omp-commands-and-completion-2026-10-30.md)。

2026-11-03 范围确认：用户选择保持官方 OMP，补齐其已导出接口的操作。未导出的任意
TUI 工厂不再作为交付前提，不安装私有 OMP 运行时；原生补全也保持实际协商结果。
本轮临时界面和语义操作边界见 [ADR-218](../decisions/providers/adr-218-native-ui-and-actions.md)。
这一确认范围的实现和验收已完成，结果见
[最终交付报告](../reports/providers/native-delivery-and-omp-roles-2026-11-02.md#2026-11-03-最终交付)。
下文完整远程 completer 与直连 ACP 的设计仍是后续提案，不作为已导出能力交付的一部分。

## 对照结论

Ait 的 Codex Provider 当前使用原生 app-server，而不是 Codex TUI 或官方 Codex ACP。
可借鉴的边界是 adapter 拥有能力映射，客户端使用 Ait 的稳定控件：

- `crates/provider/src/local/codex/discovery.rs` 查询原生模型目录和推理能力。
- `crates/provider/src/local/codex/commands.rs` 将 compact、goal 映射为原生控制，
  不把它们作为推理文本发送；技能和自定义 prompt 则只有匹配真实目录后才展开。
- 原生 tool、approval、usage、subagent 与历史通过各自投影进入 Ait。

因此应复用这种能力映射方式，不机械复用 Codex 的传输协议，也不能据此认为 Codex
已有一个能远程执行任意 TUI 补全函数的通道。

官方 OMP 18.14.0 的 ACP 支持 configOptions、会话 list/fork/resume/close、
steering 和表单交互，但 `toAcpAvailableCommands` 仍只投影 name、description、input。
其 `createAcpExtensionUiContext` 将 setStatus、setWidget、setEditorText、custom 和
addAutocompleteProvider 等方法置空；extMethod 没有动态命令补全入口。
RPC 的声明式目录保留信息更多，但也没有完整的原生补全函数接口。
标准 ACP 的当前命令输入 schema 同样不能替代原生动态多级补全。

这一结论来自源码与官方 schema 核对，不意味着本次已经完成官方 ACP 所有能力的
运行验收。直连 ACP 的切换仍需验证后台结算、子 Agent、历史、图片和交互没有回退。

## 接入原则

1. **ACP 作为客户端语义边界。** 模型、推理和模式优先使用 configOptions；命令目录
   使用 available_commands_update；对话使用 permission/form；会话操作优先使用标准
   list/fork/resume/close，不能表达时才使用明确协商的扩展。
2. **一个会话只有一个原生 owner。** 保留当前 Rust ACP 桥接与单一 OMP 进程。
   不用第二个 RPC 进程读取第一个 ACP 实例的内存状态，不并发写同一原生历史。
   官方 ACP 覆盖现有行为后，再按兼容性测试决定是否替换桥接后端。
3. **入口共享一份能力事实。** 工具栏和加号菜单展示标准配置与已实现操作；斜杠命令
   走同一套 Provider 执行路径。菜单不维护另一份命令规则，新增支持不依赖用户知道
   某条隐藏命令。未实现操作不能通过发送说明文字给模型冒充执行。
4. **Provider 内映射，客户端不认识插件。** 与 Codex 类似，原生内置操作可以在 owning
   adapter 中映射为原生 API；插件的参数规则和业务逻辑继续由 OMP 执行。
   原生控制不得绕过宿主的回合、会话、取消和审批所有权。
5. **不以定制运行时或终端面板替代接口。** 不注入 stdin 监听器，不修改原生私有对象，
   不解析终端屏幕，不复制一套 OMP 补全器，也不自动下载维护私有 OMP 分支。
   原生终端可以是辅助入口，但不计作 Ait 聊天界面已实现对应操作。

## 原生多级补全所需的最小接口

完整补全应由 **OMP 同一会话的原生补全器** 计算，包括根命令、插件回调、嵌套参数、
模型和 MCP 名称、路径、usage、ghost hint、候选顺序、可选参数与接受后的再次补全。
Ait 只提交编辑器状态并应用替换结果，不从说明文字或 usage 字符串猜语法。

建议向官方 OMP 增加一个可协商的 ACP 扩展。以下名称与字段是**提议，尚非官方 API**：

```text
initialize: _meta.ompCommandCompletion.version = 1
request:    _omp/commands/complete
params:     { sessionId, text, cursor }
result:     { supported: true, items: [
              { id, label, insertText, replaceRange: { start, end },
                description?, detail?, hint?, kind?, cursor?, triggerCompletion? }
            ] }
```

- text 为完整当前输入，cursor 和 replaceRange 明确使用 UTF-16 code units。
- 服务端复用原生 completer；必要时将仅依赖会话状态的补全上下文从 TUI 抽离，
  TUI 与 ACP 调用同一实现。不能由 ACP 再复制一棵命令树。
- 查找不会提交 prompt、调用模型、改会话或运行命令；运行中的任务仍可继续交付事件。
- JSON-RPC 关联请求，客户端短延迟查询，光标/草稿改变后丢弃过期结果；后端设置数量、
  大小和并发预算。候选为空与不支持接口是不同结果。
- 接受候选后按 replaceRange 替换并恢复光标，根据 triggerCompletion 进入下一层。
  Tab/Enter、Escape、鼠标、输入法、引号、路径和多行输入必须有实际交互回归。
- 缺少协商能力时保留当前目录补全；不能把静态首级 subcommands 标成完整原生补全。

该接口需要 OMP 官方导出目前只在进程内可调用的能力。Ait 单方面增加方法名不会使
原版 OMP 获得该能力。最小官方接口扩展比长期维护自定义 OMP 运行时更合适。

实施进度：AIT 的只读请求、原生支持协商、精确编辑及再次查询已实现，见
[ADR-212](../decisions/providers/adr-212-native-editor-completion.md)。
[上游源码补丁](../../third-party/omp/README.md)已在隔离的官方 18.14.0 源码包验证
ACP、RPC、三级插件补全及表单等待期间的查询；系统安装的 OMP 未修改。
88 个内置命令中，46 个有文本/ACP handler。后续已将公开 RPC 的其他操作映射到
语义菜单，未导出的组件工厂仍受原版协议限制，不能按内置命令数量推断远程支持程度。

## 功能入口与验收

2026-11-02 追加模型角色配置任务：

- [x] 检查 Ait 对 OMP 模型角色的配置支持：原先缺少角色配置入口。
- [x] 补齐 16 个内置角色及原生自定义角色，支持各自模型、推理档位和继承设置。
- [x] 保存并恢复选择，下一轮使用私有原生覆盖；原有全局和项目配置保持不变。
- [x] 完成官方 CLI 角色解析、恢复及继承重置的最终验收。
- [x] 修复旧显式 ACP 的进程组关闭、超时和回收，保持并发回归通过。
- [x] 补齐官方状态、widget、标题、编辑器、登录链接及 36 项语义操作。
- [x] 验证原生 Goal、fork、树导航、持久句柄及重启后的当前分支。
- [x] 缓存模型发现；OMP 按前端模型访问后台刷新，取消五分钟过期与周期发现，合并同 Host/cwd 请求并保留旧目录；隔离操作等待与草稿编辑。
- [x] 完成 Linux Electron 实际点击、错误重试、角色配置、原生 UI 和文件下载验收。

实现见 [ADR-217](../decisions/providers/adr-217-omp-model-roles.md)，
验收见[后续报告](../reports/providers/native-delivery-and-omp-roles-2026-11-02.md)。

| 能力                                   | Ait 入口与负责层                                      | 必须验证的结果                                                    |
| -------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------- |
| 模型、推理、权限、fast/slow 与原生开关 | 工具栏配置；ACP configOptions / owning adapter        | 取原生可用值，同一实例生效，不覆盖原生额外设置                    |
| 原生命令与插件                         | 斜杠目录、可发现的菜单入口；ACP prompt/命令目录       | 本地命令不调用模型，未知输入不擅自改写                            |
| 原始完整多级补全                       | 远程原生 completer + Composer                         | 与同一会话的 TUI completer 候选、顺序、替换范围和多层接受行为一致 |
| select/confirm/input/editor/ask        | 共享表单与审批                                        | 可取消、可重连、prefill/空值/secret/原文保留                      |
| status/widget/editor 控制与 URL        | 显式语义事件和相应界面                                | 更新/删除生效，不落成助手正文；命令可以完成其用户交互             |
| 会话树、分支、回退、导入导出           | 原生会话操作菜单；标准 ACP 或 typed Provider controls | 同一原生历史与身份，明确处理宿主持久句柄变化                      |
| 子 Agent 与后台任务                    | 现有子 Agent/任务界面补齐操作                         | 控制目标与祖先关系验证，后台结算不提前释放前台                    |
| MCP、工具和插件管理                    | 原生目录与对应管理动作                                | 不清除本机配置，不在客户端复制插件业务逻辑                        |
| 任意 TUI 自定义组件                    | 优先为操作提供语义化表单/动作契约                     | 能完成相同操作；不能把不能远程执行的工厂透传当成功能已支持        |

上述完整能力表保留为后续设计清单。当前确认范围、已实现操作及运行验证逐项记录在
交付报告中；未导出的任意 TUI、主题、组件工厂和完整多级 completer 不宣称已支持。

## 来源

- [OMP ACP 实现](https://github.com/can1357/oh-my-pi/blob/338d2ce6658dfdbfba19efe0f006f0d5a8edd66d/packages/coding-agent/src/modes/acp/acp-agent.ts)
- [OMP 原生命令目录](https://github.com/can1357/oh-my-pi/blob/338d2ce6658dfdbfba19efe0f006f0d5a8edd66d/packages/coding-agent/src/slash-commands/available-commands.ts)
- [OMP 原生补全器](https://github.com/can1357/oh-my-pi/blob/338d2ce6658dfdbfba19efe0f006f0d5a8edd66d/packages/coding-agent/src/slash-commands/builtin-completions.ts)
- [ACP v2 命令输入 schema](https://github.com/agentclientprotocol/agent-client-protocol/blob/main/schema/v2/schema.json)
- [现有 ACP 桥接边界](../decisions/providers/adr-190-omp-acp-settlement.md)

## Test coverage

**not measured**：本文为后续接入设计，不把提议的接口或功能列入已通过的测试。
当前已落地修复的定向测试、安装版验证、格式与 lint 结果单独记录在验证报告中。

### 原生 model preset 补充适配

- [x] 原生分层目录与后台缓存刷新，成套应用到 Ait 聊天模型、角色和默认思考强度。
- [x] 通过官方命令保存/删除指定全局条目，保留项目归属及真实失败；保存确认写盘后才关闭 reader。
- [x] 官方 18.14.0 和当前系统 18.17.0 的隔离真实运行检查，不使用源码补丁或外部模型推理。
- [x] 完成最终整合后的新增预设界面桌面验收；保存/草稿/Profile 与真实模型角色截图的不同范围见[验收报告](../reports/providers/native-delivery-and-omp-roles-2026-11-02.md#2026-11-06-至-07真实模型与新增界面)。

实现见 [ADR-221](../decisions/providers/adr-221-omp-model-presets.md)；Ait Agent Profile 与 OMP
原生 preset 保持不同保存机制，私有配置快照可通过 Profile 复用。
