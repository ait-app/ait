# DSH / OpenCode 原生命令与能力适配

日期：2026-10-30，2026-10-31 补充结算与控制隔离验证，2026-11-02 补充提交入口复核。源码：`ac8e7e33` 加当前工作树。
平台：Linux x86_64。DSH：官方 `@deepseek-ai/dsh` 0.2.1-rc.4；OpenCode：官方 2.1.3，
发布源码 `bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1`。
决策：[ADR-205](../../decisions/providers/adr-205-provider-native-command-ownership.md)。

## 2026-11-02 后续交付

命令附件、最终受理与失败恢复、原生 ZIP 下载及历史卡片恢复已补齐，官方 DSH 插件和真实
ZIP 解压通过。OpenCode 已换用实际可工作的 minimax-m3.1-flash，在官方 CLI 和 Ait
真实 daemon 两条链路识别红色图片并正常结算。完整记录与本轮 OMP 角色配置见
[后续交付报告](native-delivery-and-omp-roles-2026-11-02.md)。下方较早记录保留其当时的失败和限制。

## 2026-11-02 提交入口补充复核（后续接入前）

发现 Composer 最外层 `submitAgentInput` 仍会 trim 输入，先前草稿创建和排队内部的
原文保留不足以覆盖实际发送入口。现已补齐直接发送、运行中排队、强制发送和失败恢复：
命令参数的引号、换行、制表符及尾部空白原样保留。命令前的空白也不删除，避免将
原生解析器本来不识别为命令的文本改成可执行命令；普通消息沿用原有的 trim 规则。

另核对已安装 DSH 0.2.1-rc.4 的附件与导出契约，以下仅是源码核实，不算功能交付：

- 文件须通过 Agent 范围的 `fileUploads/upload` 或原生流式上传路由取得 staged receipt，
  再与图片一起传给 `commands/execute`，不能把 Ait 的文件路径冒充 receipt。
- `command/run` 发生在附件校验前，不能作为附件已经受理的确认。原生客户端在携带附件的
  命令最终返回 error 时保留草稿；当前 Ait 在 `command/run` 后释放执行 lane 的机制还需
  增加后续结果与草稿事务的关联，才能兼顾交互式命令和失败恢复。继续保留执行前拒绝附件。
- `/export` 的成功只表示下载请求；完整 ZIP 来自同一 Host 的认证 GET
  `/api/session.export?sessionId=…&includeDescendants=true`，包含子会话与附件。
  该二进制流还需接入 Ait 的文件交付及下载界面，不能将原生成功文字计作导出完成。

本次仅修改前端提交行为；按仓库规则不重跑既有 Rust 改动的测试。提交、排队和草稿创建
三个相关测试文件 **71 项通过**，包含本次新增的 8 项回归场景。前端类型检查、格式及
文档链接检查通过。相关 ESLint **0 错误、13 条既有警告**，均为未改动行上的数组类型写法
或重复导入；本次提交入口修改未新增警告。**Test coverage: not measured**，用户未请求覆盖率测量。

```sh
npm exec --workspace=@ait/mobile -- vitest run \
  src/composer/submit.test.ts src/composer/actions.test.ts src/composer/draft/create-flow.test.ts
npm run typecheck --workspace=@ait/mobile
npm exec --workspace=@ait/mobile -- eslint \
  src/composer/submit.ts src/composer/submit.test.ts src/composer/actions.ts src/composer/actions.test.ts
npm run check:docs
```

## 2026-10-31 桌面实操与修复

用户已授权真实使用现有 Provider 和 API。本阶段使用独立 Electron 测试配置、测试 daemon
和临时 Git 项目；实际通过桌面鼠标、键盘和控件操作。KDE 物理桌面处于锁屏状态，
用户无法到场授权，因此改在独立 X11 图形会话运行同一测试版，操作真实系统文件选择器，
不修改锁屏策略、不代填或替换系统对话框结果。截图保存在仓库外
`target/provider-parity-desktop/2026-10-31/`，不将运行数据库和凭据加入 Git。

DSH 已完成的实际路径：

- 草稿 `/` 目录显示插件命令和原生输入提示；新会话首条 `/goal` 返回原生状态，不调用模型。
- `/permission read-only` 同步工具栏；工具栏恢复 workspace-write 后真实读取 README，
  模型返回 `DESKTOP_DSH_OK`（截图 125–130）。
- 加号 → Upload file → 真实原生选择器选取 README → 提交附件 → 模型读取并返回
  `DESKTOP_DSH_FILE_OK`（131–133）。
- `/plan`、`/plan off`、目标创建、目标状态、编辑、恢复、清除及原生提问；选择 Beta 后
  工具收到答案并完成目标。`/compact` 实际压缩 29 条历史记录（134–151）。
- 根据用户反馈改为结构化目标和命令卡片，[ADR-206](../../decisions/providers/adr-206-native-control-result-cards.md)。
  暂停、编辑、继续和清除均通过真实卡片按钮操作；继续后选择 Alpha，原生目标完成，
  卡片自动变为 Complete，旧卡片移除操作（180–192）。原始多行输出收进详情。
- 只读历史修复后的新 daemon 再次恢复同一会话，编辑器正确预填且可取消；切换深色主题后
  执行原生 `/goal`，仍显示完整目标卡片，点击 Clear 后原生目标变为 Not set，旧卡片
  撤下操作（245–254）。原生文件选择器 Cancel 返回空草稿，没有新增附件（255–256）。

OpenCode 已完成的实际路径：

- 使用用户已有 futureppo 模型配置；草稿显示 ACP 推送的 init、compact、review 和测试项目
  自定义命令；Build/Plan 模式和 Ask 权限可切换（152–160）。
- 自定义命令作为首条输入由原生模板展开，真实模型返回 `DESKTOP_OPENCODE_COMMAND_OK`。
  此项使用隔离的回执修复源码，不能算作官方安装版 2.1.3 已修复（157–158）。
- Plan 模式读取 README，桌面选择 Allow once，模型返回 `DESKTOP_OPENCODE_READ_OK`。
  Build 写入请求选择 Reject，目标文件实际不存在（161–168）。
- 同一会话在重启后恢复，重新请求写入并选择 Allow once，文件内容实际为
  `OPENCODE_ALLOWED`，模型返回 `DESKTOP_OPENCODE_WRITE_OK`（195–198）。
- 新会话真实模型返回 `DESKTOP_OPENCODE_FRESH_OK`，随后 `/compact` 实际完成原生上下文
  压缩（219–223）。
- 真实图片选择器上传成功，但当前模型调用未验收通过：deepseek-v4.1-flash 返回
  `invalid_request`，glm-5.3-flash 返回原生网关“无可用渠道”；另一路配置的请求长时间
  无输出，已在桌面点击 Stop 并确认停止（199–218）。原生 CLI 对照也未得到视觉回答，
  不据此宣称 Ait 图片接入正确或把失败算作已调通。
  另在测试项目补充该 deepseek 模型的原生图片输入声明，官方 CLI 用同一 256 像素红色
  测试图重试仍返回 `invalid_request`；这不是仅缺少 Ait 模型菜单标记就能解决的问题。

本阶段修复了首条命令绕过原生 admission、跨 lane 控件丢失、空闲权限事件被过滤、
命令输入历史遗漏，以及草稿和排队命令参数被 trim 的问题。DSH 恢复已有对话不再调用
只复用空会话的 session/create，而是保留原生 ID 并通过公开模型选择入口确认并激活
原 Agent；已在真实桌面重启后操作原目标成功。这个入口仍要求对应原生模型配置可用。

随后真实并发验收又复现一次恢复拒绝。源码核实 `session/follow` 会在发送冷历史快照后
自动激活 Agent，原来的历史与列表预览因此也会竞争 writer。现已改用官方不激活 Agent 的
`session/list`、`session/projections` 和固定 `asOfSeq` 的 `session/page`；恢复前也先只读
验证 ID 和目录，见 [ADR-207](../../decisions/providers/adr-207-dsh-non-activating-history.md)。
原来失败的两项真实已有会话测试保持默认并行运行，修复后 **2 项通过**，原生记录和配置不变。
只读目录不导出创建时间，导入改用已有契约允许的观察时间，最后活动时间继续使用原生记录。

这些路径不代表当时全部能力已对齐：DSH `/export` 当时仍只有原生“请求下载”的应答，
ZIP 文件交付尚未接通；命令附件、原生业务弹层及下方列出的其他差异仍未完成。

本阶段定向验证：跨 Provider 与直接相关控制共 **359 Rust 测试通过、53 忽略**；后续
DSH 恢复和只读历史修复另跑 owning adapter，**108 通过、8 忽略**；真实已有会话并发
导入/恢复 **2 项通过**。界面卡片、流归并、缓存、布局、
提交和排队 **270 项通过**；共享控制 fixture 的协议/生成校验器测试 **3 项通过**。
UI 类型检查、相关 lint、Rust format、provider clippy 与 daemon 构建通过。
一次连接拒绝测试暴露了“释放端口后被其他并行用例复用”的竞争，已改为保留一个未监听的
绑定 socket，相关 adapter 测试重新通过，未降低并行度。

本阶段 **Test coverage: not measured**，用户未请求覆盖率；上述数量是测试执行结果。
原有 CLI 联调结果见下文，不能与本阶段使用真实 API 的桌面操作混作同一验证范围。

只读历史修复的定向命令如下；已有会话用例要求先释放测试 writer，原生 profile 所需环境
按其正常配置传入，不将凭据写入命令、报告或仓库。两项真实测试保持默认并行，不降低线程数。

```sh
cargo test --locked -q -p provider --lib -- local::deepseek_harness
DSH_HOME=/path/to/private/test-profile \
AIT_TEST_DSH_BIN=/usr/bin/dsh \
AIT_TEST_DSH_SESSION_ID=idle-released-test-session \
cargo test --locked -q -p provider --lib installed_existing_session_ -- --include-ignored
```

## 按各自原生机制实现

DSH 的 Host commands 服务提供按 Agent 分层的插件目录、直接执行事务和持久的
`command/run`、`command/done`。Ait 保留原始参数、插件身份和附件能力元数据，不发送
模型提示词替代命令。收到原生 admission 后继续交付问题与事件，不占住等待用户回答的
执行 lane。未知命令明确拒绝；模型目录为空且未显式改模型时，仍可使用原生控制。

命令、目录查询与模型执行分别拥有诊断。后台目录刷新失败保留上一份目录；主动查询
仍报告错误。命令 HTTP 响应失败显示有界的控制错误，不把正在运行的模型标为失败。
`command/done` 使用独立文本投影，避免覆盖该模型回合的最终回答；恢复历史使用同一
原生事件身份。目标等 driver 自主发布的 `turn/start` 进入 Ait 的实际运行生命周期。

OpenCode 使用标准 ACP `available_commands_update`，覆盖初始响应之后的推送、前台任务
期间更新、空闲更新和空目录。目录查询共享现有 owner，不替换事件 reader。草稿发现只
使用自己拥有且能原生删除的临时会话，不调用模型、不遗留持久空会话。命令模板、参数、
模型及 Agent 选择继续由原生 handler 处理。

| 能力       | DSH                                                     | OpenCode                                      |
| ---------- | ------------------------------------------------------- | --------------------------------------------- |
| 命令目录   | scoped commands/list，保留 definitionId、hint、附件标记 | 原生推送，保留顺序、命名空间及实际提供的 hint |
| 执行入口   | commands/execute，和当前模型任务独立                    | ACP prompt；2.1.3 结算缺陷见下节              |
| 动态更新   | commands/change 与显式刷新，空目录生效                  | available_commands_update，空目录生效         |
| 控制结果   | 原生 command/done 与历史共用投影                        | ACP 流与完成后的权威历史                      |
| 异常隔离   | 控制失败不终止模型、不污染模型诊断或最终回答            | 已知提前应答的命令在执行前拒绝                |
| 名称所有权 | clear/new/exit 等不被客户端归档动作截获                 | 同左                                          |
| 模型与权限 | 保留现有工具栏及原生 Agent preset                       | 保留现有原生 configOptions 控件               |

## OpenCode 2.1.3 的结算缺陷与上游修复

真实并行验收曾复现：自定义命令刚入队，ACP 已返回 `end_turn`；随后 Ait 回放历史，
可能连新输入都尚未出现，从而得到 `Completed(None)`。单独运行偶尔通过不能证明结算
正确。原生 ACP 提交命令时也没有传入自己生成的输入 ID，事件 translator 无法关联模型
执行。内部服务有 wait API，但该版 ACP 不导出该接口，也不支持连接外部服务。

[可审查补丁及许可证](../../../third-party/opencode/README.md)采用原生输入回执：

- 保留既有 command HTTP 接口，新增 experimental `session.submitCommand`。
- 接受原生输入 ID，返回 handler 实际接纳的 `SessionInbox.User`；纯本地 handler 仍可返回 void。
- 配置命令继续使用原始模板、参数、权限、模型和 Agent 逻辑，只传递 ID 并返回原始回执；旧 handler 的其他返回值仍被忽略，Promise 插件的序列化回执归一化为同一原生类型。
- ACP 校验 session 和输入 ID 后，等待已有的事件 consumer。取消、审批和失败沿用该原生路径。
- 保留引号内部空白、多行及参数末尾空白，不再拆词后重新拼接参数。
- initialize 协商 `_meta["opencode/command-lifecycle"] = 1`，仅用于此上游提案。

官方安装的 2.1.3 未修改。对已核实有提前应答缺陷且未协商修复能力的该版本，Ait 在
提交前拒绝受影响的斜杠输入；`/compact` 和普通提示词保留原路径。运行中不通过睡眠、
额外提示词、历史轮询、屏幕解析或第二个原生 writer 猜测完成。此限制不是“自定义命令
已经在官方安装版完整可用”的声明。

## 当前差异与未完成项

- DSH 命令图片/文件附件、staged-file receipt、最终失败时保留草稿及完整 ZIP 交付已在
  2026-11-02 后续交付中实现；只对原生命令声明接受附件的路径启用，不为未知附件种类猜测语义。
- DSH 的 Web 客户端 popupSelect、action、插件设置页和业务专属视图是客户端函数，
  不是通用 Host RPC；这些界面及所有后台任务仍需逐项接入与端到端验收。
  goal 暂停、继续、编辑和清除已接入原生命令，并在上述真实桌面流程验收。
- DSH Host 和 OpenCode ACP 都不导出通用动态参数函数；当前目录补全不冒充原始多级补全，
  也不从 usage 猜测业务参数树。
- OpenCode 2.1.3 的 availableCommands 只投影 name/description；input hint 仅在实际收到时展示。
  补丁的自定义 handler 需返回相应输入回执；后台/subagent 命令及任意第三方 handler 尚未完成完整验收。
- OpenCode 的 undo/redo、会话树、分享、终端等原生客户端操作仍有独立缺口，不能把命令目录
  当作完整 TUI。官方 ACP 文档也列出部分内置命令的支持限制。
- OMP 的远程原始补全依赖另一个上游接口；参见 [OMP 报告](omp-commands-and-completion-2026-10-30.md)。
  任意 TUI 工厂、依赖本地 onApplied 回调的候选和未导出的操作不列为已完成。

## Test execution

2026-10-31 合并定向结果：**315 Rust 测试通过，53 项默认忽略**。只选择改变的 adapter、
命令协议与直接相关执行控制；未运行完整 Rust workspace 测试。真实运行单列：**15 项通过**，
覆盖官方 OMP、DSH、OpenCode 和两份隔离上游修复源码；不把后者算作官方发布能力。

```sh
cargo test --locked -q -p provider --lib -- \
  local::omp local::deepseek_harness local::opencode \
  protocol::commands service::agent_execution::tests::controls

AIT_TEST_OMP_BIN=/usr/bin/omp \
AIT_TEST_OMP_COMPLETION_BIN=/path/to/isolated-omp-with-completion \
AIT_TEST_DSH_BIN=/usr/bin/dsh \
AIT_TEST_OPENCODE_BIN=/usr/local/bin/opencode \
AIT_TEST_OPENCODE_COMMAND_BIN=/path/to/isolated-opencode-with-command-receipts \
cargo test --locked -q -p provider --lib -- \
  local::omp::tests::installed local::opencode::tests::installed::commands \
  installed_host_registry_and_native_goal_status_work_without_a_model_prompt --include-ignored
```

真实 DSH 使用临时 DSH_HOME，无凭据查看 `/goal`。OpenCode 使用临时 XDG、数据库与
loopback mock 模型；修复版用例仍要求最终权威回答，官方版用例要求受影响命令在原生执行前
被拒绝。未访问个人 DSH 历史、余额接口或付费模型。

OpenCode 上游源码：既有 prompt/turn 与新增回执回归合计 **23 项通过**（22 项组合运行后，
新增跨 Session 回执用例与其余 6 项回执用例一起通过）；原生命令服务/模板 **37 项通过**，包含旧 handler 返回值兼容和 Promise 插件回执归一化。
CLI/Core 包的定向类型检查和 21 个变更文件的 lint 通过。验证使用其声明的 Node 类型
24.12.2，避免解析到外层 Ait 的类型依赖。上游全仓检查不计作通过：额外全仓类型检查尝试
在当前嵌入式 Bun 的 Turbo 执行器中异常退出；以上仅为实际完成的定向检查。

前端 **72**、协议 **14**、SDK **19** 项测试通过；SDK build、移动端类型检查、UI lint、
Rust format、provider clippy、Cargo workspace check 和文档链接检查通过。Clippy 仅沿用
`assert_is_empty` 的工具链豁免。workspace 仍有既有 filesystem `fetch_update` 弃用 warning。

本地复核日志保留在共享 Cargo target 的 `provider-parity-validation/2026-10-31/`，源码
验证环境在 `provider-parity-native/`；均不是要提交的运行工件。Windows、macOS、已安装
桌面包的人工交互验收未执行。

## 原生依据

- [DSH 官方命令服务](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/interaction/commands)
- [DSH 客户端命令](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/client/ui-commands)
- [DSH automation ACP profile](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/bundle/acp-app)
- [OpenCode 2.1.3 ACP 目录与变更](https://github.com/anomalyco/opencode/blob/bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1/packages/cli/src/acp/sessions.ts)
- [OpenCode ACP 提交与结算](https://github.com/anomalyco/opencode/blob/bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1/packages/cli/src/acp/turn.ts)
- [OpenCode 原生命令解析](https://github.com/anomalyco/opencode/blob/bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1/packages/cli/src/acp/prompt.ts)
- [OpenCode ACP 官方范围与限制](https://opencode.ai/docs/acp/)
- [OpenCode 自定义命令](https://opencode.ai/docs/commands/)

## Test coverage

**not measured**：用户未请求覆盖率测量。上述是测试执行结果，不是行覆盖率。若后续需要，
可对本报告的 provider filters 执行 `cargo llvm-cov` 并提供可复核工件；不扩大为 workspace
全量覆盖率。
