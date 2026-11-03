# OMP 命令、参数补全与交互兼容验证

- 日期：2026-10-30。
- 源码：`ac8e7e33` 加本次工作树修改。
- 平台：Linux x86_64，官方 `/usr/bin/omp` 为 `omp/18.14.0`，Rust 1.99.1。
- 决策：[ADR-203](../../decisions/providers/adr-203-omp-command-catalog-and-completion.md)、[ADR-204](../../decisions/providers/adr-204-native-editor-completion.md)。

## 2026-11-02 模型角色配置

已补齐 16 个内置角色和原生自定义角色的模型、推理档位选择、继承与持久恢复；
使用官方 OMP 的只读目录及每次运行配置覆盖，无需补丁。记录见
[原生交付与角色配置报告](native-delivery-and-omp-roles-2026-11-02.md)。

## 2026-10-31 真实桌面实操

用户已授权现有 API 和模型的真实调用。本阶段运行独立 Electron 测试配置、测试 daemon 和
临时 Git 项目，实际通过桌面控件、鼠标和键盘操作。截图位于仓库外
`target/provider-parity-desktop/2026-10-31/`；系统安装的 OMP 18.14.0 未修改。

- `/advisor st` 补全至 `/advisor status`，执行后得到原生 Advisor 状态。
- 真实 Tai 模型读取 README，返回 `DESKTOP_OMP_OK`。
- 工具栏切换 Always ask，提交写文件请求，在原生权限卡片中选择 Deny 并提交；文件实际不存在。
- 另一写入请求选择 Approve，随后批准原生字节校验，实际文件恰为 `OMP_ALLOWED`，
  模型返回 `DESKTOP_OMP_WRITE_OK`（233–241）。模型首次多写换行后继续修正，未将初次回答当作验收。
- 加号 → Upload file → 原生文件选择器选取 README，真实模型读取上传附件并返回
  `DESKTOP_OMP_FILE_OK`（237–243）。
- 使用测试项目的原生扩展，select 选择 Beta；input 提交保留首尾空格；editor 保留多行和空格；
  空字符串与 Dismiss 区分，confirm 的同意/拒绝返回 true/false；secret 输入实际遮蔽，
  只输入测试假值后取消（061–073、088）。
- 测试源码提供的原始动态补全连续显示 nested → second → third，Tab 应用原生替换结果
  （082、090–091、228–231）；完整提交后原生回调确认参数为 `nested second third`（232）。
  这使用隔离上游提案源码，不冒充官方 RPC 已导出该能力。
- 重启恢复后再执行 confirm，桌面只展示原生标题、说明和选项，提交后回调为 true；
  不再展示重复 RPC JSON 或无内容的详情占位（225–227）。
- 连续本地命令后真实模型返回 `DESKTOP_AFTER_COMMANDS_OK`，其用户输入身份与当前提交回执相同。

桌面实操修复：补齐 Rust desktop transport 的 complete 方法映射；补全查询期间保留
Tab/Enter 焦点及 Escape→Enter 的即时状态；原生支持但无候选时关闭菜单；冷启动请求
预算增至 30 秒；桌面 daemon 保留明确 allowlist 中的 Provider 启动配置。OMP 本地命令
未产生用户消息时清理自己的请求身份，避免把下一条模型输入误标为旧命令；confirm 卡片
不再重复展示原始 RPC JSON。

`/custom-provider` 已真实提交，官方 RPC 的 `ctx.ui.custom()` 返回 undefined，插件静默退出。
该任意 TUI 工厂缺口仍未解决，不能列为“已调通”。系统文件选择器原先受 KDE 锁屏和权限框
阻挡；随后在独立图形会话中已实际操作同一 Ait 的原生文件选择器，并完成 DSH 附件输入，
没有修改用户锁屏或代填选择器返回值。

本阶段直接相关验证：OMP adapter **92 通过、13 忽略**，桌面 transport **45 项通过**、
两个补全 hook **17 项通过**，desktop 环境传递 **4 项通过**，冷补全 timeout 的 SDK 回归
**1 项通过**。随后跨 Provider 的定向验证与新的结构化控制卡片结果见
[DSH / OpenCode 桌面报告](dsh-opencode-capability-adaptation-2026-10-30.md)。
**Test coverage: not measured**，未请求覆盖率；测试执行数量不是覆盖率。

## 本次修复

截图所示的参数候选缺失来自两层信息丢失：Rust 将原生命令目录中的 aliases 和
subcommands 丢弃，Composer 又将命令名后的空格视为补全结束。

现在命令元数据贯穿 OMP adapter、ACP、共享 wire schema、SDK 和 Composer。
`/advisor o` 根据原生目录显示 `on`、`off` 与说明，不在 Ait 硬编码 advisor 或其他插件。
补全按原生顺序与前缀筛选；原生 usage 作为参数提示显示。根命令名称仍使用 Ait 现有搜索排序。

| 行为                                 | 当前实现与验证                                                                                              |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| 原生命令名称、别名、source、参数提示 | 从官方目录读取；不改写插件或技能名称                                                                        |
| 声明式子命令参数                     | 保留 name、description、usage，支持根命令及其别名                                                           |
| 键盘、鼠标与光标                     | 方向键选择、Tab/Enter 接受；完整参数后的 Enter 提交；中途补全保留后续文本和光标；Escape 关闭并保留 OMP 草稿 |
| 根命令前导空白                       | 按可执行命令查找，不误判为正文中的技能引用                                                                  |
| 已有会话目录                         | 查询所属执行 lane 的原生目录，包含请求同时提供 draftConfig 的情况；不会因列目录另起 OMP                     |
| 原生目录更新                         | 同一实例的 available_commands_update 更新缓存；空目录生效；标准 ACP 同步发布更新；菜单重开刷新              |
| 草稿/无运行实例                      | 保留现有只读 Provider 发现回退；不自动创建交互会话                                                          |
| 客户端命令冲突                       | OMP 的 clear/new/exit/quit/q 经原生发送/队列链路处理，不执行客户端归档动作；原生是否支持某个命令由 OMP 决定 |
| input/editor                         | 可提交空字符串；保留原文空白；取消仍回传 cancelled                                                          |
| editor                               | 显示 prefill，可原样提交或清空；保留缩进和首尾换行                                                          |
| secret 输入                          | 按原生标记使用密码控件；答案不裁剪                                                                          |
| 其他 Provider                        | 新字段可选，保留现有命令响应和客户端命令规则                                                                |

命令发现与本地命令不会伪造模型回合，也不保证 OMP 已写入尚未产生 assistant 消息的历史文件。
command_output 继续使用现有展示投影，不能仅凭 Ait 的展示条目类型推断是否调用了模型。

## 基础阶段 Test execution

使用 `CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target` 复用编译缓存。

```sh
AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -q -p provider --lib -- \
  local::omp service::agent_execution::tests::controls \
  --include-ignored --skip local::omp::tests::real
```

**105 项通过**：OMP adapter、ACP、命令目录与更新、原生回合结算、问答、审批、历史、
恢复，以及直接相关的执行 lane controls 回归。包含 9 项真实安装 CLI 测试，使用临时配置、
通用测试插件及 loopback mock 模型；新增真实 `/advisor status` 与别名、子命令目录验证。
两个 `local::omp::tests::real` 付费推理测试显式排除，不访问用户模型凭据。

```sh
npm exec --workspace=@ait/mobile -- vitest run \
  src/utils/agent-command-autocomplete.test.ts \
  src/hooks/use-agent-autocomplete.test.tsx \
  src/hooks/use-agent-commands-query.test.ts \
  src/client-slash-commands/index.test.ts \
  src/components/question-form-card-core.test.ts \
  src/components/question-form-card.test.tsx
npm run test --workspace=@ait/protocol -- \
  src/messages.omp-commands.test.ts src/messages.omp-dialogs.test.ts \
  src/provider-manifest.omp.test.ts
```

前端/表单测试 **65 项通过**，协议测试 **11 项通过**。测试使用共享命令、交互 JSON 夹具，覆盖原始 metadata、Zod、生成的
AOT validator、实际 hook 的键盘选择和提交、过期候选保护、编辑器预填及密码控件。
前端测试在 jsdom 中运行，尚未在截图对应的桌面安装包进行人工验收。

## 静态检查

```sh
npm run build:sdk
npm run typecheck --workspace=@ait/mobile
npm run check:ui
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings -A clippy::assert_is_empty
cargo check --locked --workspace
npm run check:docs
git diff --check
```

上述检查通过。workspace 编译仍报告既有 `filesystem/workspace_runtime.rs` 的
`AtomicUsize::fetch_update` 弃用 warning，与本次 OMP 修改无关，未扩大修改范围。
Clippy 仅沿用已有验证对 Rust 1.99 新增 `assert_is_empty` lint 的豁免；其他 warning
仍视为错误。未修改其他 Provider 的既有测试来绕过该工具链差异。
未运行完整 workspace 测试，未替换或重启用户正在使用的 Ait/OMP。

## 仍存在的兼容边界

本次不能宣称已达到 OMP TUI 的全部功能与行为一致。边界分别是协议未提供的信息、
Ait 尚未提供的界面，以及尚未执行的平台验收：

| 差异                                                   | 原因与后续工作                                                                                         |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| 扩展 getArgumentCompletions、动态模型/MCP/文件参数补全 | 官方安装版没有远程接口；AIT 接入与隔离源码补丁已验证，见下方第二阶段；仍需要支持该接口的官方运行时     |
| optional 子命令与部分 inline/ghost hints               | RPC 目录未包含 TUI 的 subcommandOptional、动态 hint 回调；当前仅依据已返回的目录补全                   |
| 任意 TUI 组件、主题、footer/header/custom editor       | 官方 RPC 不提供远程组件工厂，无法按终端布局直接复现                                                    |
| setStatus/setWidget/set_editor_text/open_url           | 原生扩展通道可向协商客户端传帧；Ait 聊天界面尚未完整实现这些编辑器/状态/浏览器控制，透传不代表已可操作 |
| 分支树、fork/rewind、原生队列编辑与子 Agent 独立控制   | 仍受现有 Ait 生命周期与控制入口限制；当前会话/文件回退能力继续声明不支持                               |
| 宿主工具/URI 注册、每会话 MCP/工具策略覆盖             | 外部 ACP 原生扩展通道与 Ait 宿主能力不是同一层；保持原有未实现边界，原生本机 MCP/工具照常加载          |
| macOS/Windows、已安装桌面包                            | 未执行；2026-10-31 已补充独立 Linux Electron 测试版与真实用户 API 的桌面操作，范围见本文开头           |

官方依据：[RPC 协议](https://github.com/can1357/oh-my-pi/blob/main/docs/rpc.md)、
[命令目录构建](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/slash-commands/available-commands.ts)、
[TUI 补全实现](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/slash-commands/builtin-completions.ts)。

## Test coverage

**not measured**：用户未请求覆盖率测量。本次只运行改动和直接相关行为的定向测试；
测试通过数量不代表行覆盖率。若后续要求测量，可针对这些 provider filters 使用
`cargo llvm-cov`，生成可复核报告，不扩展为 workspace 全量测试。

## 原生动态补全阶段

在 ADR-203 的目录回退之外，增加 `agent.commands.complete.request`、SDK
`agent.completeCommands({text,cursor})` 和 Composer 原生编辑接入。已有会话查询同一
owner，OMP 原生支持通过 ready capability 协商；旧版明确不支持且不发送未知命令。
已确认缺少能力时复用协商结果，不在每次输入时重新启动 CLI；再次发现可更新能力。
原生查询等待命令发现结束，避免草稿的两个能力读取同时启动。
支持的空结果不回退，错误会显示；候选与完整输入、UTF-16 光标关联，原生替换和结果
光标原样应用，接受后继续查询下一层。输入/输出大小、候选数量、ID 和代理对边界有验证。

上游提案及许可证见 [third-party/omp](../../../third-party/omp/README.md)，基于
`v18.14.0` / `31147f9d1a76608549821c3759f8487b4d66c72d`，`git apply --check` 通过。
TUI 与远程入口共用命令构建及原始补全函数；远程入口排除仅本地界面可执行的命令。
该发布版 88 个内置命令中，46 个有文本/ACP handler；其余包含计划/目标/循环、会话树、
原生登录面板、Git、主题和本地编辑器等在该阶段尚未提供语义操作。
2026-11-03 用户确认保持官方 OMP，仅补齐已导出接口；这些公开操作现已接入并完成
Linux Electron 验收，见[最终交付报告](native-delivery-and-omp-roles-2026-11-02.md)。
未导出的任意组件、主题及完整原生 completer 仍不宣称已支持。

源码验证与系统安装版分开：系统 `/usr/bin/omp` 未修改，增强运行时来自隔离的官方 npm
源码包，使用系统可执行文件内嵌的 Bun。`AIT_TEST_OMP_COMPLETION_BIN` 仅指向该测试入口。
真实 ACP 验证扩展协商与 `/advisor`；RPC 验证原生内置补全、任意三级插件回调、空结果，
并验证一个命令正在等待原生选择表单时仍能查询，回答表单后原命令继续完成。

```sh
AIT_TEST_OMP_BIN=/usr/bin/omp \
AIT_TEST_OMP_COMPLETION_BIN=/tmp/ait-omp-with-completion \
cargo test --locked -q -p provider --lib -- \
  local::omp protocol::commands service::agent_execution::tests::controls \
  --include-ignored --skip local::omp::tests::real
npm exec --workspace=@ait/mobile -- vitest run \
  src/hooks/use-agent-autocomplete-native.test.tsx src/hooks/use-agent-autocomplete.test.tsx
npm run test --workspace=@ait/protocol -- \
  src/messages.omp-commands.test.ts src/messages.omp-dialogs.test.ts src/provider-manifest.omp.test.ts
npm run test --workspace=@ait/client -- src/index.test.ts -t 'native completion'
```

最近定向执行为 **114 Rust、14 hook、13 协议、1 SDK 测试通过**，已包含独立的 controls 补全用例与辅助函数整理。上游补丁额外 3 项
Bun 测试通过，验证原生嵌套参数回调、引用/后缀保留和 UTF-16 拒绝边界。

AIT SDK build、移动端类型检查通过；Provider clippy 使用前述单项工具链豁免通过。
上游源码包的类型检查存在与未修改同版 npm 源码基线相同的 10 项错误（ambient const
枚举、signals 类型和 number/bigint），本补丁未新增类型错误；不能声称整个上游包检查
通过。生成 RPC wire artifacts 与原生源码运行验证通过。

本阶段 coverage **not measured**：未请求测量；测试通过数不代表行覆盖率。仍未运行
完整 Rust workspace 测试，也未进行桌面安装包人工验收。

## 2026-10-31 合并复核

与 DSH、OpenCode 的命令适配一起定向运行：**315 Rust、72 前端、14 协议、19 SDK 测试通过**；
另有 **15 项官方 CLI / 隔离源码联调通过**，涵盖 OMP 原始三级插件补全与表单等待期间的查询。
OMP 上游 3 项补全测试再次通过；真实 provider-factory 用例还验证无文本/光标变化的候选被省略，避免 Enter 陷入重复补全。RPC wire artifacts 重新生成；19 文件的补丁补充了协议说明、
Unreleased 记录，并通过对固定发布源码的应用检查。定向源码 lint 通过。

这些是测试执行数量，coverage 仍 **not measured**。官方 OMP 未修改；依赖本地 onApplied
回调的候选会被远程入口排除，不能把该提案称为任意 TUI 行为已经完整远程化。
详细合并范围、命令和剩余差异见 [DSH / OpenCode 报告](dsh-opencode-capability-adaptation-2026-10-30.md)。
