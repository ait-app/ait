# Workspace Bot 重设计：每个 Workspace 一个管理者

- 状态：已批准，阶段 1 已发布（0.1.30）、阶段 2 已实现（[ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)）、阶段 3 已实现（[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）；阶段 4 的通知与唤醒已实现（[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)），剩余事项见下文“阶段 4 剩余工作”；
  计划外新增的任务、指派与汇报已实现（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md)），剩余事项见下文“任务、指派与汇报”（实施计划：[阶段 1](../archive/plans/workspace-bot-manager-phase-1.md)、[阶段 2](../archive/plans/workspace-bot-manager-phase-2.md)、[阶段 3](../archive/plans/workspace-bot-manager-phase-3.md)）
- 日期：2026-11-05
- 关系：已由 [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、[ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)、[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 实施：取代 [ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md) 的多 Bot／人格模型、
  修订 [ADR-141](../decisions/clients/adr-141-experimental-workspace-bots.md) 的标签页与
  [ADR-145](../decisions/bot/adr-145-bot-permission-denial.md) 的权限规则；现行行为见 [Workspace Bot](../operations/bots.md)。

## 意图

Bot 是 Workspace 的**管理者**，也是 Ait 之外（Telegram、飞书）进入这个 Workspace 的联络点。
用户只说想法和目标，不说实现细节；Bot 理解 Workspace 全貌，把想法变成 Agent 任务或定时任务，
跟进结果并回报，同时记住用户的长期偏好（如“PR 标题用中文”“回复简短口语”）。

典型场景：

1. **随手交办**：在 Telegram 说“用户反馈登录页闪退，修一下”。Bot 阅读相关代码和历史、形成计划，
   启动一个普通 Ait Agent 去修，跟进后回报发现、改动、测试结果和 PR 链接。
2. **周期性工作**：说“每天 22:00 做一次 release”。Bot 不自建调度，而是在 Workspace 中创建一个普通
   Cron 定时任务指向 release 会话，并自行写好提示词和 Cron 表达式；用户可在 Ait 中像其他任务一样查看、暂停、编辑。
3. **跟进通知**：22:00 的 release 失败时，Bot 被通知唤醒，查看失败原因，在对话中写下
   “release 卡在测试步骤，疑似不稳定用例 X，要重跑吗？”。

当前实现的问题：一个 Workspace 可有多个带人格的 Bot；Bot 是被隔离的聊天会话，所有权限请求自动拒绝，
不能实际管理 Agent，也不感知 Workspace 中发生的事情。

## 目标

- 每个 Workspace 至多一个 Bot，以 `workspaceId` 为身份；Bot 标签页是与它对话的唯一入口。
- Bot 能读取 Workspace（代码、git 状态、文档、Agent、定时任务、脚本、通知），
  通过 Ait MCP 启动、提示、观察、取消 Agent 并收集结果，管理定时任务。
- 权限在设置中一次性配置，运行中**从不**申请额外权限。
- Bot 可接入 Telegram 与飞书，在 Ait 之外交办任务、接收回报。
- Bot 有内部记忆，记录用户偏好与长期约定，并在每次回复及交办给 Agent 时遵循。
- Workspace 的通知可以唤醒已配置的 Bot，结果追加到对话。

## 非目标

- Bot 不直接修改代码或 Workspace 文件；任何改动都通过它启动的 Agent 完成。
- 不自建调度器、Agent 运行循环或通知系统，复用 Workspace 定时任务、Provider 会话与 attention。
- 不迁移现有多 Bot 数据：旧 Bot 直接丢弃（功能仍处于实验阶段）。
- 不在本轮实现 Email Channel、流式事件订阅或富媒体／卡片回复。

## 反目标（技术上可用但算失败）

- **未配置 Bot 的 Workspace 产生任何模型调用或外部 API 请求。**
- Bot 在 Telegram 发起的任务中停下来等待一个没人会看到的审批。
- Bot 绕过设置的权限：通过回答子 Agent 的审批、伪造调用者身份或调用不在授权范围内的 MCP 工具扩大能力。
- Bot 对 Workspace 中每条通知都跑一次模型，成本失控或刷屏。
- Bot 启动的 Agent 在 Ait 中不可见、无法接管，或丢失“由 Bot 启动”的来源信息。

## 用户已确定的决策

1. **一个 Workspace 一个 Bot**，旧的多 Bot／人格模型连同数据一起删除，不做迁移。
2. **整个新 Bot 功能继续放在 `experimentalFeatures` 开关之后。**
3. **Bot 是管理者，不写代码。** 它可以读代码、做计划、启动 Agent 和执行各类管理操作，但不直接改动 Workspace。
4. **权限由用户一次性配置，之后 Bot 不再申请更多权限**；超出范围的操作被拒绝而不是等待审批。
5. **Bot 启动的 Agent 是普通的原生 Ait 会话**，权限可以高于 Bot（用于改代码和跑测试），
   遵循普通 Agent 的权限规则，在 Ait 中可见、可接管。
6. **定时任务复用 Workspace 现有 Cron 定时任务**，Bot 负责把自然语言转成任务配置。
7. **Bot 可访问该 Workspace 的全部通知**，并可被通知唤醒，在内部会话执行一轮，结果追加到对话。
8. **生命周期**：新建 Workspace 没有 Bot；任何人在任意设备打开 Bot 标签页并完成一次配置后，
   Bot 即处于可用状态，此后即使所有标签页都关闭，仍响应外部 Channel 与通知。
   **未配置的 Workspace 不得触发任何内部 LLM 会话或 API 调用。**
9. **记忆**用于记录用户偏好，例如 PR 标题语言、与用户说话的风格。
10. **Bot 启动的 Agent 的 Ait MCP 继承 Bot 的权限限制**，Bot 的设置对整棵子树都成立。
11. **Workspace 归档即永久移除 Bot 并删除其记忆。**

## 设计

### 生命周期

| 状态   | 进入方式                   | 行为                                                                               |
| ------ | -------------------------- | ---------------------------------------------------------------------------------- |
| 未配置 | Workspace 创建后的默认状态 | 不存在 Bot 记录、隐藏会话和 Channel；不订阅通知；零模型与零外部请求                |
| 活跃   | 在 Bot 标签页完成首次设置  | 响应 Ait、Telegram、飞书消息；按唤醒规则响应通知                                   |
| 暂停   | 设置中暂停                 | 与现行语义一致：取消排队与执行中的输入，停止通知唤醒和外部轮询，零模型调用；可恢复 |
| 退役   | 设置中移除 Bot             | 停用 Channel、退役隐藏会话、保留历史；之后可重新配置为新的 Bot，可选沿用记忆       |
| 删除   | Workspace 归档             | 同退役，并**删除 Bot 记忆**；Workspace 无法取消归档，Bot 不可再配置                |

打开 Bot 标签页本身不创建任何东西；只有提交首次设置才创建 Bot。标签页关闭不影响 Bot 状态。
Workspace 目录缺失（如磁盘未挂载、worktree 被移动）时 Bot 进入**暂停**而非退役，目录恢复后由用户恢复，避免丢失设置。
归档与目录缺失不同：归档是用户结束 Workspace，Bot 随之永久移除；目录缺失只暂停。
暂停与退役不影响 Bot 已启动的 Agent 和已创建的定时任务，它们是普通 Workspace 资源。

### 身份与数据模型

- Bot 以 `workspaceId` 唯一定位，协议与标签页不再携带 `botId` 列表、名称或人格。
  内部可保留 `botId` 作为存储与 generation 键，但一个 Workspace 同时只能有一个非退役 Bot。
- 设置项：
  - **Bot 自身模型**：Provider／模型／推理级别。
  - **权限**（见下节）。
  - **启动 Agent 的默认配置**：默认 Provider／模型／模式，以及 Bot 允许选择的模式上限。
  - **外部 Channel**：Telegram、飞书，沿用现有凭据引用与允许列表。
  - **记忆**：可在设置中查看、编辑。
- 删除旧的 `instructions`（人格）字段；Bot 的系统指令由 Ait 内置（管理者角色、工作方式、权限说明），
  用户风格偏好进入记忆。

### 能力与权限

Bot 隐藏会话的能力分三层，均在执行端强制，而不是依赖提示词：

1. **原生工具：只读。** Bot 会话使用 Provider 能提供的最严格只读模式（如 Codex 只读沙箱、
   Claude 只允许读取／搜索类工具），可以读代码和运行只读的检查命令，但不能写 Workspace 文件。
   其余原生权限请求沿用 ADR-145 自动拒绝，绝不等待审批。
2. **Ait MCP：按授权分组开放。** Bot 会话注入 `ait` MCP，工具按以下分组授权，分组之外的工具不出现在
   Bot 的工具目录中，调用也会被 MCP 路由拒绝：

   | 分组             | 包含                                                                                                                                                                                                                                                                           | 默认       |
   | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------- |
   | 观察             | `list_agents`、`get_agent_status`、`get_agent_activity`、`wait_for_agent`、`list_schedules`、`inspect_schedule`、`schedule_logs`、`list_workspace_scripts`、`list_terminals`、`list_profiles`、`list_providers`、`list_models`、`inspect_provider`、`list_pending_permissions` | 开（必选） |
   | 启动与指挥 Agent | `create_agent`、`send_agent_prompt`、`update_agent`、`set_agent_mode`                                                                                                                                                                                                          | 开         |
   | 停止 Agent       | `cancel_agent`、`archive_agent`、`kill_agent`                                                                                                                                                                                                                                  | 开         |
   | 定时任务         | `create_schedule`、`update_schedule`、`pause_schedule`、`resume_schedule`、`run_schedule_once`、`delete_schedule`、`create_heartbeat`、`delete_heartbeat`                                                                                                                      | 开         |
   | 脚本             | `start_workspace_script`、`stop_workspace_script`                                                                                                                                                                                                                              | 关         |
   | 终端             | `create_terminal`、`send_terminal_keys`、`capture_terminal`、`kill_terminal`                                                                                                                                                                                                   | 关         |
   | 隔离工作区       | `create_workspace`（仅创建当前项目下的 worktree Workspace）                                                                                                                                                                                                                    | 关         |

   永远不授权：`respond_to_permission`、`archive_workspace`、`rename_workspace`，以及作用于其他 Workspace 的任何操作。

3. **Bot 专属工具**：记忆读写与 Workspace 概览（见下文），只对 Bot 身份暴露。

**作用域**：Bot 的所有 MCP 调用限定在自己的 Workspace（以及它创建的 worktree Workspace）。
**控制范围**：观察覆盖 Workspace 内全部 Agent 与定时任务；指挥和停止类工具同样可作用于用户自己启动的 Agent
（Bot 是整个 Workspace 的管理者），但所有改变执行权限的入口都受模式上限约束。

**模式上限**：各 Provider 的模式是互不可比的字符串，因此上限按 Provider 配置为**允许的模式列表**
（向导给出默认值：每个 Provider 允许其默认模式及更保守的模式）。上限同时约束 `create_agent`、
`set_agent_mode`、`update_agent`，以及 `create_schedule`／`create_heartbeat`／`update_schedule` 中的新 Agent 配置，
防止通过定时任务加 `run_schedule_once` 绕过。未在设置中列出的 Provider 不可由 Bot 使用。

**身份必须可验证。** 现有 `X-Ait-Agent-Id` 只是编排上下文，不构成权限边界（ADR-148）。
Bot 会话注入时使用**绑定到该 Bot 的独立 MCP 凭据**，MCP 路由据凭据识别 Bot 并应用上述授权与作用域；
凭据随 daemon 重启和 Bot generation 轮换，不写入命令行、持久 handle 或 Workspace 文件（沿用 ADR-202 的保密规则）。
共享 Agent MCP 令牌在任何携带 Bot 会话 ID 的请求上被拒绝，防止退回无约束路径。

**修改权限只在设置中进行**，在 Bot 下一轮生效（不要求新对话），不能在聊天中授予。

### Bot 启动的 Agent

- 由 Bot 通过 `create_agent` 启动的 Agent 是普通、可见的 Ait 会话：出现在 Workspace Agent 列表，
  可打开、观察、接管，遵循普通权限规则（可能停在 Ait 等待用户审批）。
- 记录来源：Agent 标记“由 Workspace Bot 启动”，以及发起它的 Bot 对话轮次，供 UI 展示与 Bot 跟进。
  **注意**：现有 attention 计算会排除 internal 会话并按父链寻找根（`workspace_attention.rs`），
  若直接把隐藏 Bot 会话记为父级，子 Agent 会从 attention 中消失。Bot 启动的 Agent 必须被视为
  Workspace 的根级 Agent，来源用独立标签记录，而不是 internal 父级。
- Bot 不回答子 Agent 的审批。子 Agent 等待审批时，该通知唤醒 Bot（见通知），Bot 告知用户
  “某某 Agent 正在 Ait 中等待你的审批”，用户在 Ait 中处理。
- Bot 在给 Agent 的提示中带上记忆中的相关偏好（如 PR 标题语言）。
- **Ait MCP 继承 Bot 的限制（用户已确定）。** Bot 启动的 Agent，以及 Bot 创建的定时任务／心跳所启动的 Agent，
  注入的 `ait` MCP 使用**派生凭据**：工具分组与作用域与 Bot 相同（同样永不包含 `respond_to_permission`），
  这些 Agent 再启动的子 Agent 继续继承，形成整棵子树的上限。原生权限（改代码、跑测试）不受影响，
  仍按普通 Agent 规则和模式上限执行。共享 Agent MCP 令牌不得注入到这棵子树中的任何会话；
  MCP 路由需按凭据识别“Bot 子树”并执行检查，凭据随 daemon 重启轮换，Bot 设置变更后在下一次请求生效。
  用户在 Ait 中自己启动的 Agent 不受影响。

### Workspace 全貌

每轮执行前，Bot 获得一份紧凑的 Workspace 概览（作为轮次上下文，而非写入文件）：

- 由 Bot 启动且未归档的 Agent 及其状态、最近结果摘要；Workspace 内其他 Agent 的数量与需要关注的条目。
- Bot 创建的定时任务与最近运行结果；其他定时任务的简表。
- 自上一轮以来的 Workspace 通知。
- 当前分支、未提交改动概况。
- 记忆全文。

概览有大小上限，超出部分让 Bot 通过观察类 MCP 工具按需查询。概览由 daemon 现有读模型拼装，不新增缓存或存储。

### 定时任务

Bot 把“每天 22:00 发布”之类的请求转成 `create_schedule`（Cron、IANA 时区默认取用户偏好或设备时区），
目标为“每次新建 Agent”或某个已有会话，提示词由 Bot 根据记忆与上下文写好。
任务是普通 Workspace 定时任务，归属记录“由 Bot 创建”，用户可直接在 Ait 编辑；Bot 不另存一份调度状态。

### 通知与唤醒

- **来源**：Workspace attention（Agent 完成、失败、等待审批）、定时任务运行结束与失败、终端 attention。
  Bot 能看到 Workspace 内全部通知（进入概览）。
- **唤醒范围**：默认只有 _Bot 启动的 Agent_ 与 _Bot 创建的定时任务_ 的通知会触发一轮执行；
  设置中可改为“Workspace 全部通知”。其他通知只进入下一轮概览，不调用模型。
- **未配置或暂停的 Bot 不订阅通知**，不产生任何调用。
- **合并**：通知进入持久化的 Bot 收件队列；在一个短时间窗口内合并为一次唤醒，
  Bot 正在执行时排队合并到下一轮，避免一连串完成事件触发多次模型调用。
- **输出路由**：唤醒轮次在**发起该工作的对话**中执行并回复（在 Telegram 交办的任务，结果回到该 Telegram 会话）；
  每个 Agent／定时任务记录发起它的对话。来源不在任何对话中的通知（用户自启的 Agent、全部通知模式）进入 Ait 主对话。
  收件队列按对话分组，沿用同一对话串行执行。Bot 可判断无需告知（普通完成且无后续动作），只记录不推送。
- 同一 Workspace 的所有对话共享记忆与概览，Ait 标签页可只读查看外部对话，因此在 Ait 中也能看到外部回报。

#### 阶段 4 剩余工作

截至 2026-11-12，收件队列、合并唤醒、按发起对话路由、无需告知、唤醒范围设置、Workspace 事件源与 Bot 标签页的展示已实现，
用户可见行为见 [Workspace Bot](../operations/bots.md#通知与唤醒)。合入前的修复轮已处理：Bot 已等待并回报的 Agent 不再重复唤醒、
卡住的对话不再占满收件箱、被重启打断的唤醒轮次重试、关闭与重启时的定时任务结果不再丢失、去重覆盖重放窗口，
以及定时任务 Agent 的审批请求与心跳成功结果不再重复通知（[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 5、11、13、14 节）。尚未完成：

- **真实平台验收**：唤醒回复在 Telegram、飞书、QQ、微信上的投递只有模拟测试，尚无人用真实账号验证；
  QQ 主动消息配额与飞书不在话题内回复的实际体验也未确认。移动端与桌面界面未在真机上检查。
- **Ait 中只读查看外部对话**：Bot 标签页只显示主对话，回到 Telegram 等平台的唤醒轮次在 Ait 中看不到。
- **复用的定时任务 Agent**：`archiveOnFinish: false` 的定时任务 Agent 之后被复用时仍带运行标签，其通知不再产生。
- **重启后的观察**：“已观察”只保存在内存中，daemon 重启后同一结果可能再唤醒一次；尚未决定是否值得持久化。
- **降级**：有活动的 Workspace 升级后实际上无法降级到不支持通知的 daemon（ADR-271 第 12 节），需决定是否提供导出或清理工具。
- **对话中的可点击条目**：唤醒轮次的通知头可打开 Agent 与定时任务，Bot 回复中提到的 Agent 与定时任务仍是纯文本。
- **浏览器端到端用例**：`workspace-bots.spec.ts` 尚未覆盖唤醒轮次。

### 任务、指派与汇报

原计划没有这一阶段，在阶段 4 之后加入。通知唤醒之后，Bot 仍倾向于在一轮中 `wait_for_agent` 等 Agent 结束，阻塞对话，
完成的工作也无人收尾。现在 Bot 交办的每项工作是一个持久的任务（工单）：Bot 启动的 Agent 自动加入任务，
工作者用 `report_progress` 汇报，Bot 交出工作后即结束本轮，在 Agent 结束、等待审批或汇报时被唤醒，告诉用户结果后关闭任务；
用户在 Bot 标签页查看任务，并可关闭、取消或重新打开。契约见 [ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md)，
用户可见行为见 [Workspace Bot](../operations/bots.md#任务)。

截至 2026-11-13，daemon（记录、存储、状态机、通知挂接、作用域 MCP 工具与 `create_agent` 的任务流程、概览、访客规则）与客户端（协议、SDK 特性
`workspaceBotTickets`、对话中的任务入口与详情、唤醒卡片的任务链接、九种语言）已实现，由单元、集成与组件测试覆盖。尚未完成：

- **真实会话验收**：端到端测试用伪执行器代替模型；真实 Provider 会话中 Bot 是否交出工作后结束本轮、是否要求 Agent 汇报并关闭任务，
  以及 Agent 是否调用 `report_progress`，尚未在运行中的 daemon 上验证。任务界面未在真机、桌面应用或浏览器端到端用例中检查。
- **遗留任务**：创建 Agent 前崩溃留下的无 Agent 任务不自动清理；Bot 忘记关闭的任务只占用 32 个名额，由用户手动关闭。
- **关闭与停止**：关闭或取消任务不停止它的 Agent，需决定是否提供一并停止的操作。
- **定时任务**：定时任务运行创建的 Agent 不属于任何任务。
- **跨 Workspace 的 Agent**：作用域内其他 Workspace（旧 worktree）中的 Agent 在任务详情中不能打开；Agent 名称只在客户端已加载 Agent 目录时显示。
- **访客**：只有整个 Host 的访客能读取任务；Workspace 与项目访客看不到。
- **降级**：任何 Agent 汇报过 `blocked`、`done` 或 `failed` 后，降级到 0.2.10 需要删除 `<data>/workspace-bots/`（ADR-274 第 13 节）；
  daemon 也不能容忍以后新增的任务枚举值。

### Channel 与对话

- 保留内置 Ait Channel、Telegram、飞书，以及现有的持久接纳、`messageId` 去重、uncertain 恢复规则。
- **主对话**：Ait 标签页显示的对话；没有发起对话的通知唤醒结果写入这里。
- 外部 Channel 中的每个发送者／线程仍是独立对话（沿用现行隔离），但共享同一份记忆和 Workspace 概览，
  因此在 Telegram 交办的任务，在 Ait 中问 Bot 也能知道。Ait 标签页可只读查看外部对话。
- 用户发来的消息无论标签页是否打开都会得到回复（只要 Bot 处于活跃状态）。

### 记忆

- 用途：用户偏好与长期约定（语言、风格、PR 规范、发布流程习惯等），不保存任务流水。
- 记忆按 `workspaceId` 保存，跨 Bot generation 保留；退役后重新配置时，向导提示沿用或清空旧记忆。
- **Workspace 归档时删除记忆（用户已确定）**，与 Workspace 归档的其他清理一起执行，不留下无主数据。
- Bot 通过专属工具 `bot_memory_read`／`bot_memory_update`（带 revision）自行增删；用户在设置中查看和编辑。
- **存储位置迁出 Workspace 目录**，改为 daemon 数据目录下按 `workspaceId` 保存，
  使 Bot 不写 Workspace 文件，删除 worktree 也不会悄悄丢失记忆。上限沿用 32 KiB。

### Bot 标签页（客户端）

- 实验开关开启时，Workspace 新建菜单中的 Bot 项打开**该 Workspace 唯一的** Bot 标签页（已有则聚焦）。
- 未配置：显示设置向导（模型 → 权限 → 启动 Agent 默认值 → 可选外部 Channel）；提交即创建 Bot。
- 已配置：对话视图 + 设置入口。对话中 Bot 启动的 Agent、创建的定时任务以可点击条目呈现。
  通知唤醒的轮次以区别于用户消息的样式标识来源。
- 实验开关关闭：隐藏入口；已恢复的标签页显示启用提示，不发起 Bot 查询；关闭开关不暂停服务器上的 Bot（沿用 ADR-141）。
- 协议以新的能力名（如 `workspace-bots-v2`）协商；旧 Host 显示升级提示。

### 删除旧模型

- daemon 升级后丢弃旧版 Bot 配置、Channel、会话映射和对话历史（含其隐藏 Provider 会话的退役），不做迁移。
  旧记忆文件位于 Workspace 目录内，不自动删除用户文件；在发布说明中注明可手动清理 `.ait/bots/`。
- 移除多 Bot 列表、创建（多实例）、人格编辑等协议方法与界面；`bot.permission.resolve.request` 等兼容入口随 v1 一并删除。

## 约束

- Rust 实现，保持 `bot → domain/model/provider` 依赖方向；MCP 授权在 `api` 的 MCP 路由执行，
  Bot 身份与授权表由 bot 能力通过端口提供，`provider` 不依赖 `bot`。
- 不新增外部数据库、消息队列或辅助进程；复用文件持久化。
- 凭据仍只保存环境变量引用；Bot 专属 MCP 凭据不落盘。
- 只支持具备 HTTP MCP 注入的 Provider（当前为 Codex、Claude、DSH ACP、MiniMax Code）作为 Bot 自身模型；
  被启动的 Agent 不受此限制。
- 实施时新增 ADR、更新 `docs/operations/bots.md` 与 `docs/README.md`。

## 设计者的取舍（待用户复核）

以下是讨论中未明确、由设计者做出的决定：

1. Bot 的 MCP 授权采用固定分组，脚本、终端、创建 worktree Workspace 默认关闭。
2. 用独立的 Bot MCP 凭据作为权限边界，而不是信任 `X-Ait-Agent-Id`。
3. 通知默认只唤醒 Bot 自己发起的工作，可在设置中扩大到全部通知；唤醒前做短窗口合并。
4. 唤醒结果回到发起该工作的对话（Telegram 交办则回 Telegram），无来源的进入 Ait 主对话。
5. 外部发送者保持独立对话，共享记忆与概览；不把 Telegram 私聊并入 Ait 主对话。
6. 记忆迁到 daemon 数据目录，按 Workspace 保存，通过 Bot 专属工具读写；重新配置时可选沿用。
7. 模式上限按 Provider 列出允许的模式，覆盖所有能改变执行权限的入口（含定时任务）。
8. Bot 可指挥、停止 Workspace 内所有 Agent，包括用户自己启动的。
9. 目录缺失时暂停而非退役。
10. 旧 Bot 的记忆文件不自动删除。
11. 子树中的 Agent 若在 Ait 中被用户接管，仍保留派生凭据，直到会话重新建立；不在运行中切换为共享令牌。

## 留给实施者

- 各 Provider 只读模式的具体映射与验证。
- 概览的字段裁剪、大小上限与通知合并窗口时长。
- 收件队列的持久化格式与重启恢复（沿用现有输入回执与 uncertain 规则）。
- `workspace-bots-v2` 的方法清单与 TypeScript 协议／SDK 更新。
- 定向测试范围：未配置零调用、权限分组与作用域拒绝、凭据隔离、子 Agent attention 可见性、
  通知合并与唤醒范围、暂停／退役、旧数据丢弃。
