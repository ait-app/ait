# 原生附件、ZIP 交付与 OMP 模型角色配置

日期：2026-11-02 开始，补充复验至 2026-11-06。范围：各节标明的提交及当前工作树；Linux x86_64。

## DSH 与 OpenCode 补充交付

DSH 0.2.1-rc.4 的原生命令现支持 capability 声明允许的图片和文件附件。先预检所有附件，
文件经同一 Agent 的认证流式上传取得真实 receipt，再提交原生 commands/execute。
最终原生拒绝会返回发送失败，客户端保留草稿；等待期间 lane 仍可交付问答和审批。
创建流程同样等最终受理，不提前显示创建完成。定向测试还修复了维护线程处理排队输入时
缺少 observer fence 的状态读取竞争，未降低测试并行度。
重启时也先为恢复的队列建立 owner，再接受读取请求，避免读到未唤醒的关闭状态。
原生模式元数据无效时保留正确的静态回退，不让旧 owner 缓存重新覆盖它。

官方 DSH 首次组合 profile 在临时目录实测会超过 30 秒。启动预算已与控制请求分开：
启动 60 秒，已连接控制仍为 30 秒。定向测试使用一秒的慢启动和更短的控制预算，验证
慢启动成功且后续超时请求仍失败；没有通过降低测试并行度或延长测试等待来掩盖问题。

原生 `/export` 成功后读取同一 Host 的认证 ZIP 流，验证大小、MIME 和 ZIP 完整边界，
只在完整文件落地后更新原生结果卡。点击卡片下载走现有认证文件通道；重启读取历史仍可
恢复下载入口。失败和取消不会留下可下载的半成品，也不追加伪造的助手消息。
官方 DSH 普通临时插件实测精确文件字节、图片读取、原生命令及真实 ZIP 解压通过。

OpenCode 2.1.3 图片路径已使用可工作的 `futureppo/minimax-m3.1-flash` 实测：
相同 256 像素纯红色图片在官方 CLI 和 Ait 的真实 Rust daemon、共享客户端 transport
两条链路均回答 `Red`，Ait 最终状态 idle。此前 deepseek 的 invalid_request 和渠道失败
保留为失败记录，不计入成功。该轮是 CLI 与真实 API 验证，没有冒充桌面点击验收。
私有运行证据在 `target/provider-parity-validation/2026-11-02/`，不提交凭据和运行日志。

## OMP 模型角色

原先缺少角色配置，现已实现。设置中的 `modelRole.*` 选择覆盖：
default、smol、slow、vision、plan、commit、tiny、memory、task、advisor、widget、
image、web、speech、dictation、judge，以及 OMP 配置、标签和 cycleOrder 中的自定义角色。
每个角色可选目录中的模型及其支持的推理档位，也可恢复为沿用原生设置。

角色选择保存到 Ait 会话配置并从下一轮生效，运行中请求沿用已受理配置。
OMP 通过官方每次运行的 `--config` 接收稀疏覆盖，用户全局和项目文件不变；
恢复已有会话时重用原生日志。当前聊天模型与各用途角色独立配置。
图像、搜索、语音、转写等模型只进入适用的角色候选。
尚未恢复原生进程的会话通过只读查询补全候选，保留已保存模型和推理档位。

## 2026-11-03 最终交付

用户确认的范围已完成：保持官方 OMP 18.14.0，补齐已导出的语义操作、模型角色、
旧显式 ACP 关闭和新增界面的 Linux Electron 验收。没有安装私有 OMP 运行时。

- 旧显式 ACP 关闭直接发送进程组信号，先合作退出，再有界强制回收；回收预算从实际
  发出信号时起算。虚拟时钟反例覆盖延迟调度，16 会话并发关闭和共享进程清理均通过。
  冷启动预算与已连接控制请求分开；没有降低测试并行度。
- 官方状态、文本 widget、标题、编辑器、登录链接和 36 项语义操作已有界面入口。
  操作覆盖会话树、分支、fork、handoff、新会话、命名、标签、HTML 导出、账号、
  自有子任务、旁路问答、投递模式、缓存预热、重试、shell、Host 语音与听写，以及
  原生 Goal 的 get/create/resume/pause/drop。字段按目录校验，不透传任意 RPC。
- 操作的草稿、结果和等待状态分别保存；失败保留输入和旧结果，可以切换其他操作。
  等待原生确认不占住审批及问答通道；编辑器只等待本 Agent 的提交事务。
  临时状态、widget、编辑器和登录链接不写对话历史或离线缓存。
- 树导航使用官方当前叶节点投影历史，持久句柄保存定位。恢复时通过官方接口重定位；
  关闭产生的 `session_exit` 及模型设置等元数据保留定位，真正的外部续写使旧定位失效。
  只读历史返回同一有效定位，不修改原生日志。此前官方重启用例失败的原因已修复。
- OMP 成功模型目录不按时间过期；前端模型相关页面进入、菜单打开/选择、查看与诊断按访问
  触发后台刷新。同一 Host/cwd 的并发访问合并，冷启动加入已有发现。刷新期间与失败后保留
  最后成功目录及时间戳；缓存按目录和环境隔离并有界淘汰。此策略取代早期五分钟过期方案。
  创建会话和角色设置复用有效目录，不在每次打开菜单时启动原生发现。可见面板定期
  检查，隐藏面板停止定时请求；角色标签变化也会更新内容版本。
- Electron 文件下载改为受信窗口专用 IPC，使用原生下载流及保存对话框，不导航应用
  页面、不将整份文件缓存在 renderer。只有写盘完成才显示成功，连接、并发、停滞、
  取消和窗口关闭均有清理；用户选择保存位置的时间不计入连接等待预算。

边界见 [ADR-218](../../decisions/providers/adr-218-native-ui-and-actions.md)。

## 最终桌面验收

使用当前工作树构建的真实 daemon、Electron 主进程及共享前端，连接官方 OMP 18.14.0、
DSH 0.2.1-rc.4。普通测试扩展只调用官方公开接口；模型初始回合使用本地协议桩，
后续角色、目标、会话与文件操作均由原生运行时执行，不调用外部模型。

证据保存在私有运行目录 `target/provider-parity-validation/2026-11-02/`，不提交运行数据：

| 验收路径           | 结果与截图                                                                                                                                                                                                                                                 |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 模型角色           | `desktop-acceptance-EPrfxv/02-role-settings.png`、`03-advisor-choices.png`、`07-native-role-resolution.png`：16 个角色及 custom，advisor 模型与推理档位保存，下一轮原生解析一致；随后重置继承并设置 custom，原生解析再次核对。原生配置及模型文件字节未变。 |
| 临时 UI            | `desktop-acceptance-tOQ2nd/16-native-ui-after-role-reset.png`、`17-native-ui-cleared.png`：真实状态、上下 widget、标题和编辑器预填显示，删除后用户草稿保留。                                                                                               |
| 独立等待与错误恢复 | `desktop-acceptance-EPrfxv/10-independent-pending-operation.png`、`11-native-rejection-preserves-drafts.png`、`12-native-error-retry-success.png`：shell 等待时编辑草稿并切换统计，原生标签拒绝后保留输入，修正后成功。                                    |
| 后台刷新           | `desktop-acceptance-tOQ2nd/18-cached-roles-and-responsive-draft.png`：刷新应答实测 23 ms，旧四个模型仍可选，菜单打开 265 ms，草稿保留；单次观测不作为通用性能承诺。                                                                                        |
| 目标及会话         | `desktop-acceptance-tOQ2nd/20-native-goal-lifecycle-complete.png`、`21-native-fork-committed.png`、`23-tree-restored-after-daemon-restart.png`：目标创建、暂停、继续、清除，fork 身份变更，树导航及 daemon 重启后的叶节点一致。                            |
| HTML 下载          | `desktop-acceptance-tOQ2nd/14-native-html-downloaded.png`：实际点击并经 Electron 写盘，46529 字节，与原生导出文件摘要一致；解码内嵌会话数据后包含真实原生回复。                                                                                            |
| DSH 附件与 ZIP     | `desktop-acceptance-tOQ2nd/26-dsh-native-attachment-receipt.png`、`29-dsh-zip-downloaded-complete.png`、`31-dsh-restored-zip-download-complete.png`：上传图片和文件，原生插件读取精确字节，ZIP 实际下载、解压，并在 daemon 重启后从历史再次下载。          |

ZIP 为 733 字节，含 `session.v4.jsonl` 和 `DSH_NATIVE_ATTACHMENT_OK` 原生回执。
两次下载 SHA-256 均为 `ca6aa922b1f3f4a0624d451d935d07c417e9ee4e9255bdde78f7161331c2a1cf`。
DSH 首次恢复因隔离配置没有可用模型而被拒绝；草稿和附件保留。补齐仅指向本机关闭端口的
测试模型配置后重试成功，没有放宽原生模型校验或更改产品逻辑。daemon 重启均正常退出，
无需强制终止；最终桌面没有 page error。

账号 OAuth 长等待与设备操作由协议及组件回归覆盖；未执行真实账号登录退出、麦克风录音
或付费语音请求。macOS、Windows 和发布安装包不属于本次 Linux 开发版桌面验收。

## 最终 Test execution

- 收尾定向 Rust：**168 通过、0 失败、15 忽略**，覆盖 OMP、旧 ACP、元数据进程、
  DSH 旧传输、模型目录缓存、延迟执行与相关取消路径。此前混合运行的失败已修复并复验。
  最后提取历史元数据 helper 后，直接相关历史回归另 **5 通过**，不与前述数量累加。
- 官方 CLI 联合验收：**3 通过、0 失败**，默认并行，20.05 秒。覆盖 OMP 原生 Goal、
  fork、树导航及重启，模型角色及继承恢复，DSH 原生附件与 ZIP/历史读取。
- Electron 下载及 IPC：**18 通过**；共享下载状态：**6 通过**。覆盖完成前保持等待、
  原生中断、取消、窗口隔离、并发限制、超时和失败显示。
- 前端缓存定时回归 **10 通过**；快照与角色候选 **24 通过**；原生操作、临时 UI、
  角色候选组件 **8 通过**。各范围有重叠，不相加作为总数。
- provider Clippy、Rust 格式、daemon 与 Electron 主进程构建、桌面及共享前端类型检查、
  相关 ESLint/oxlint、格式和文档检查通过。沿用仓库 `assert_is_empty` Clippy 例外；
  daemon 构建仅有 filesystem 已有的 `fetch_update` 弃用警告。
- 未运行完整 Rust workspace 测试或覆盖率。

收尾 Rust 使用独立共享构建目录 `target/provider-parity-build`：

```sh
cargo test --locked -q -p provider --lib -- \
  local::omp local::acp_transport local::metadata_process \
  local::deepseek_harness::tests service::provider_catalog \
  rpc::agent_execution::deferred local::opencode::tests::acp_cancel \
  local::minimax::tests::missing_cancel_acknowledgement
AIT_TEST_OMP_BIN=/usr/bin/omp AIT_TEST_DSH_BIN=/usr/bin/dsh \
  cargo test --locked -q -p provider --lib -- --ignored \
  installed_omp_native_actions_commit_forks_tree_navigation_and_reopen \
  installed_omp_roles_resolve_inside_native_extensions_and_survive_reopen \
  installed_host_admits_native_image_and_file_receipts_and_delivers_real_zip
```

## PR 与最新主分支整合

提交前将功能提交 `083112e8` 与 `main` 的 `850e0075` 合并。保留主分支新的异步
Provider trait、封闭操作路由、只读子 Agent 登记和共享读取缓存；新原生操作继续持有
对应 Agent 的 owner，延迟 receipt 不占用该 owner 的交互处理。新 ADR 编号调整为
211–218，保留主分支已有 ADR 及文档分类。

整合回归发现命令目录读取暂存的 OpenCode 空闲事件未交付给新子 Agent 处理器，
导致两项后台权限测试超时。已先消费同一连接的暂存帧，再读取新帧，并保留目录推送。
没有增加等待预算或减少并行度。修复后扩大到直接相关模块的定向检查：

- Rust：**548 通过、0 失败、58 忽略**。范围包括 OMP、DSH、OpenCode、旧 ACP、
  元数据进程、模型目录、执行路由、延迟回执与 owner；不是全 workspace 测试。
- 最后按 Clippy 拆分函数、固定较大创建 Future 后，OpenCode 与执行/RPC 直接相关回归
  **253 通过、0 失败、33 忽略**；与前述范围重叠，不累加。
- 官方 OMP/DSH 联合复验：**3 通过、0 失败**，默认并行，13.91 秒。
- 共享前端：**73 通过**；Electron 下载、IPC 和启动环境：**22 通过**。
- 相关协议：**12 通过**；SDK 原生补全：**1 通过、18 未选中**。
- provider 全目标 Clippy、Rust 格式、daemon 构建、协议/SDK 构建、共享前端和桌面类型检查、
  Electron 主进程构建、相关 ESLint/oxlint
  及文档链接检查通过。桌面截图与手动端到端结果对应前一节的整合前工作树。

```sh
cargo test --locked -q -p provider --lib -- \
  local::omp local::deepseek_harness local::opencode local::acp_transport \
  local::metadata_process service::provider_catalog service::agent_execution \
  rpc::agent_execution service::agent_manager::ownership
```

## 阶段一 Test execution

- 最终 owning scope：**325 通过、21 忽略**，包括 OMP、DSH native、执行控制及输入受理。
- 随后的 OMP 顺序探测调整：**104 通过、14 忽略**。与上一范围重叠，不累加测试数量。
- 最终官方 CLI 联合复验：**4 通过、0 失败**，默认并行，耗时 138.66 秒。
  OMP 18.14.0 三项覆盖发现、模型能力、原生角色解析、继承重置和持久恢复；角色用例先通过
  本地 HTTP 模型协议桩形成真实日志，未调用外部模型。DSH 0.2.1-rc.4 一项覆盖冷启动、
  文件及图片上传、原生执行、真实 ZIP 解压和历史恢复。
  合并复验暴露的 OMP 冷 profile 并发探测问题，通过顺序读取同次探测的配置、目录和
  运行时能力修复；OMP owning scope 和上述四项联合检查均已重新通过。
- 客户端组件及角色候选恢复：**8 通过**；相关协议：**3 通过**。前端类型检查通过。
- Provider Clippy、Rust 格式和 daemon 构建通过；Clippy 仅沿用仓库已有的
  `assert_is_empty` 例外。相关 ESLint **0 错误、1 条原有未使用变量警告**；构建仍有
  filesystem 的原有 `fetch_update` 弃用警告。
- 本地文档链接检查通过。未运行完整 Rust workspace 测试或覆盖率。

```sh
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target \
  cargo clippy --locked -q -p provider --all-targets -- -D warnings -A clippy::assert_is_empty
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target \
  cargo test --locked -q -p provider --lib -- \
  local::omp local::deepseek_harness::native service::agent_execution rpc::agent_execution
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target \
  AIT_TEST_OMP_BIN=/usr/bin/omp AIT_TEST_DSH_BIN=/usr/bin/dsh \
  cargo test --locked -q -p provider --lib -- --ignored \
  installed_omp_roles installed_omp_tiers_and_thinking installed_omp_discovers_and_opens \
  installed_host_admits_native_image
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target \
  cargo build --locked -q -p daemon --bin daemon
npm run typecheck --workspace=@ait/mobile
npm run test --workspace=@ait/mobile -- \
  src/components/provider-command-card.test.tsx \
  src/composer/agent-controls/localization.test.tsx \
  src/composer/agent-controls/role-choices.test.ts
npm run test --workspace=@ait/protocol -- \
  src/messages.provider-command.test.ts src/messages.dsh-commands.test.ts
npm run check:docs
```

阶段一更宽的父模块筛选曾出现旧显式 ACP 分支的关闭用例失败
`local::deepseek_harness::tests::image_input_is_capability_gated_and_foreign_session_updates_fail`，
失败点是 close 返回 Failed。阶段一未修复、不计为通过；现已在上述最终交付中修复并
纳入 168 项收尾回归。阶段一开发服务打包超时也没有计为桌面通过，最终真实桌面证据另列。

## 边界

本次没有把任意原生终端 UI 工厂变成通用表单。OMP 原始动态补全的上游协议条件，以及
DSH popupSelect/业务页面、OpenCode 未导出的终端操作仍见
[原生能力差异](dsh-opencode-capability-adaptation-2026-10-30.md#当前差异与未完成项)。
模型角色配置本身使用官方已安装 OMP，无需补丁或改动原生安装。

## Test coverage

**not measured**：用户未要求覆盖率测量。测试通过数量单独记录；若后续要求测量，使用
`cargo llvm-cov` 限定 provider adapter 和本轮直接相关执行模块，并附可共享报告。

## 2026-11-04：模型目录按访问刷新

按用户修订取消 OMP 五分钟成功目录过期与前端周期检查。所有模型相关入口先读缓存，访问时
异步刷新；聊天、新建 Agent、紧凑布局、角色与思考菜单、Profile、Bot、定时任务、元数据模型
和诊断复用同一 Host/cwd 的后台请求。权限模式等无关元数据读取不触发模型刷新。
刷新不删除旧目录、不改变用户当前选择，也不等待刷新才能继续输入或提交已有有效选项。
冷启动加入正在进行的原生发现，失败保留旧值，下次访问可重试；适配器有界缓存仍限制内存。

早期验收的“五分钟定时”测试数字仅是历史记录；当前按访问行为以本节补充回归为准。

本轮回归：Provider 目录 30 项、OMP 模型角色 9 项通过；最新前端缓存/快照、模型菜单、角色、
命令中心贡献、定时任务等 93 项通过，另 Profile/诊断/表单相关 57 项通过（存在重叠，不累加）。
类型检查、oxlint、Provider 全目标 Clippy、格式、daemon 构建及文档链接检查通过。

最新构建的 Linux Electron 补充验收：在隔离原生模型文件中新增 `cache-access-proof`，重新打开
模型菜单，原有 `main` 仍可见，新模型通过后台刷新出现，未出现 page error。证据：
`target/provider-parity-validation/2026-11-02/desktop-acceptance-PZsb4s/33-model-access-refreshed-cached-options.png`。
该检查针对真实官方 OMP 18.14.0，不等待五分钟、不清空目录；测试模型文件会恢复。

输入防阻塞补充证据：同一桌面测试在原生目录仍返回 `refreshing: ["omp"]` 时，成功编辑
消息输入框，草稿保持为 `Typed while OMP model refresh runs`，没有 page error；截图
`desktop-acceptance-PZsb4s/36-model-refresh-input-editable.png`。

## CI 认证恢复竞争修复

提交 `bbec9eb3` 的 CI 中，3293 项 Rust 测试有一项 Host-link 认证恢复失败：已排队的
票据拒绝与就绪的租约续期竞争，随机选择可多次先续期，最终超过测试的十秒终止预算。
收紧断言后本地复现为四次续期而非预期两次。现优先处理排队的传输错误，同时保留旧 generation
过滤及现有超时，避免持续认证拒绝被正常续期拖延；没有延长超时或降低测试并行度。

Host-link 的 44 项测试通过；该恢复用例以 8 个并行进程重复 500 次，零失败。
此修复只改变已就绪错误的处理优先级，不扩展授权或 Provider 边界。覆盖率仍未测量。

## 评审补充修复：混合版本、目录更新与有效改动范围

- 原生控制结果在共享传输出口转为旧 `assistant_message` 封装，结构化事实放在可选元数据中；
  内部日志保留 `provider_command`。元数据不重复正文，实时、子 Agent 和分页共用转换，游标及身份不变。
  用基线 `850e0075` 的真实源码复验，Goal 与 ZIP 两个样例的实时消息和完整历史页全部通过旧 schema，
  而裸新类型仍被基线拒绝。固定输入见 `provider_command_compat.json`。
- 活动 OMP 会话的配置校验和控制菜单使用当前共享角色目录；新增模型/自定义角色、刷新后选择、
  持久保存及下一轮原生配置覆盖均有定向回归。
- OpenCode 2.1.3 只保护已注册且缺少生命周期回执的命令。绝对路径、未知斜杠文本与 `/compact`
  走普通原生路径，错误仍保留。
- 前端按 Host 的补全方法能力决定是否探测；方法缺失与 `not_implemented` 回退静态目录，
  真正的原生错误继续可见。
- 目录刷新由一个 Host/cwd 维护者轮询；等待者按各自 Provider 完成，不与其他 Provider 串行等待。
  完整候选校验只留在原生适配器，内部传递 `ValidatedCompletion`，bridge 不再序列化往返。
- 两份未交付的上游补丁及许可证从本 PR 移出，独立本地提案分支
  `proposal/native-provider-interfaces-20261104` 的提交为 `82ba8c84`。当前说明只保留固定上游版本、
  协商、降级与限制；历史提案链接不代表普通用户已有此功能。

本轮覆盖率 **not measured**：未请求；如需测量，将限定 Provider 相关模块，测试通过数量不代替覆盖率。

评审修复验证：Provider 定向 219 通过、15 忽略；追加旧投影缓存字节预算的 8 项回归通过，
两组存在重叠，不累加。前端相关 210 项通过；追加共享等待失败/取消的快照测试 26 项通过；
协议结果/兼容封装 3 项通过。基线源码检查同时通过 Goal、ZIP 的实时与整页历史校验。
Provider Clippy、前端类型、协议/SDK 构建、oxlint、格式和文档链接检查通过。

2026-11-06 追加原生 model preset 适配：已实现分层目录、成套私有配置应用、模型/角色/思考强度
同步、最新下一轮选择的全局预设保存和删除。目录复用原模型缓存；模型变更时保留同 Host/cwd
的候选和草稿，失败也不清空。预设删除后会话仍能通过已保存快照恢复。
原生命令等待最终回执，保存还要确认全局条目实际写盘，避免原生异步写入尚未完成就关闭进程。
旧版本未导出相应命令时拒绝操作，不把未知斜杠输入发给模型。见 [ADR-221](../../decisions/providers/adr-221-omp-model-presets.md)。

预设追加检查：OMP 与配置补丁 134 通过、16 忽略，后续管理专项 5 通过；前端 41 通过。
官方 18.14.0 发行二进制经官方 SHA256SUMS 验证，预设专项 1 项通过；系统官方 18.17.0 的
角色/预设专项 2 项通过。上述检查范围有重叠，不累加为覆盖率。类型、Clippy、格式与文档检查通过。
这一早期追加检查尚未包含新增预设界面的桌面验收；最终整合后的真实模型与预设界面证据见下文，
不以此前 loopback 验收冒充本轮真实模型调用。
本轮覆盖率 **not measured**：未请求；如需测量，限定 OMP、配置补丁及对应前端范围。

补充：官方 OMP 18.14.0 的模型角色解析、继承重置与重新打开真实专项 1 项通过；原生 UI/卡片
组件 8 项通过。使用隔离 profile 与 loopback 模型，不调用真实账户模型或修改用户全局配置。

## 2026-11-06：完整宿主命令审计

对应 [ADR-222](../../decisions/providers/adr-222-omp-headless-commands-and-side-questions.md)。
审计固定官方 OMP 18.14.0 源码
[`31147f9d`](https://github.com/can1357/oh-my-pi/tree/31147f9d1a76608549821c3759f8487b4d66c72d/packages/coding-agent/src/slash-commands)
及已核对发行摘要的官方二进制，不使用系统中可能有补丁的副本。完整注册表包含 **88 个根命令、
9 个别名、136 个声明子命令**，包括单独导入的 `seance` 与计算得到的三个 compact 模式；
此前临时记录中的 78/87 均不是完整数量。官方 RPC 目录实际导出其中 46 个 builtin。

### 根命令与别名的完整分组

下表覆盖全部根命令；“允许”不是任意参数已实测成功的承诺。只读形式使用独立控制通道，
其他原生写操作仍经过普通调度和原生校验。未知新增 builtin 默认拒绝，真实第三方 owner 优先。

| 策略                      | 官方根命令                                                                                                                                                                                                                                                                                                  |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 允许的 26 个文本 handler  | advisor、export、dump、todo、session、jobs、usage、changelog、tools、context、mcp、compact、retry、memory、pin、marketplace、plugins、reload-plugins、add-dir、remove-dir、dirs、forget-approvals、skillful、extended-context、computer、ratchet                                                            |
| 宿主冲突或外部服务：20 个 | security、model、switch、fast、slow、prewalk、modelpreset、effort、trace、share、browser、stats、ssh、fresh、shake、handoff、rename、move、wt、force                                                                                                                                                        |
| 未导出 TUI handler：41 个 | settings、setup、plan、plan-review、vibe、goal、guided-goal、loop、queue、collab、join、leave、copy、open、hotkeys、extensions、agents、git、local、hub、branch、fork、tree、login、logout、new、clear、delete、seance、resume、tan、omfg、cleanse、debug、exit、restart、skills、live、record、pause、quit |
| 专门 RPC 适配：1 个       | btw；文本 TUI handler 不透传，改用官方 btw / btw_cancel / get_btw_history                                                                                                                                                                                                                                   |

九个别名逐项跟随实际 owner：`setup → providers`、`model → models`、
`extensions → status`、`hub → subagents`、`branch → rewind`、`wt → worktree`、
`force → force:`、`quit → q` 拒绝；`plugins → plugin` 保留。
真实 extension/custom 注册同名 `/usage` 或 `/advisor status` 时不套用 builtin 的只读名单。
绝对路径、未知普通斜杠文本和参数中的同名值不被误判为根命令。

### 子命令审计

所有 136 项声明均按其根 handler 审阅；根命令拒绝时其全部子命令也拒绝。允许根命令中另拒绝：
`advisor configure`；`session delete/pin`；`todo edit/expand/collapse`；
`mcp unauth/smithery-login/smithery-logout/reconnect/notifications`。
这些入口分别依赖终端界面或绕过宿主会话/账号所有权；菜单、手工输入和已协商补全均使用同一边界。
保留其他原生子命令及参数，不声称逐个执行了 destructive、网络安装、OAuth 或设备操作。
裸 `/advisor` 会切换开关，**不是**并行只读查询；`/advisor status/dump` 才进入独立控制通道。
`/prewalk` 会切换用途模型，`/trace` 会启动统计服务，不能因它们导出了文本 handler 就放行。

### 官方二进制安全执行：27 个形式

通过隔离 HOME、原生配置、无凭据环境和独立 cwd，禁用工具、扩展、技能和上下文发现。
仅提供指向本机关闭端口的模型元数据使 RPC 能启动，没有运行模型服务器或 mock 模型，
没有发送推理请求。以下 **26 个允许根命令及 plugin 别名**均实际经过官方 prompt handler，
回执全部为 `agentInvoked: false`，未观察到模型启动；这属于原生 handler 审计，**不是 LLM 验收**。

| 实际输入                                                    | 实际结果及限制                                                                                                                                                               |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/advisor status`                                           | 返回 advisor disabled。                                                                                                                                                      |
| `/export audit.html`                                        | 本轮 ephemeral 审计明确返回不能导出 in-memory session；持久会话 HTML 成功证据见上文“最终桌面验收”：Electron 实际写盘 46,529 字节并与原生导出摘要一致。不把本次拒绝计为成功。 |
| `/dump`                                                     | 返回当前原生 transcript；不公开完整系统提示或运行数据。                                                                                                                      |
| `/todo`                                                     | 返回当前没有 todos。                                                                                                                                                         |
| `/session info`                                             | 返回真实隔离 session ID 与 cwd。                                                                                                                                             |
| `/jobs`                                                     | 返回没有后台任务。                                                                                                                                                           |
| `/usage show`                                               | 返回零用量报告。                                                                                                                                                             |
| `/changelog last 1`                                         | 返回 18.14.0 发布记录。                                                                                                                                                      |
| `/tools`                                                    | 返回实际启用工具目录；没有执行工具。                                                                                                                                         |
| `/context`                                                  | 返回实际上下文用量。                                                                                                                                                         |
| `/mcp list`                                                 | 返回没有已配置 MCP server。                                                                                                                                                  |
| `/compact snapcompact unexpected-focus`                     | 明确拒绝不允许的 focus 参数；不算完成压缩。                                                                                                                                  |
| `/retry`                                                    | 返回 Nothing to retry；没有伪造可重试回合。                                                                                                                                  |
| `/memory view`                                              | 返回 memory payload 为空。                                                                                                                                                   |
| `/pin ait-audit-nonexistent-session`                        | 明确返回 session not found；没有更改用户会话。                                                                                                                               |
| `/marketplace help`                                         | 返回原生帮助，不访问 marketplace。                                                                                                                                           |
| `/plugins list`、`/plugin list`                             | 两种形式均返回没有安装插件。                                                                                                                                                 |
| `/reload-plugins`                                           | 返回 Plugins reloaded，作用域仅隔离 profile。                                                                                                                                |
| `/add-dir <隔离目录>/extra`、`/remove-dir <隔离目录>/extra` | 另行实际添加再移除临时目录，两次 `/dirs` 分别确认存在与消失；只修改隔离 profile，四个回执均 `agentInvoked:false`。                                                           |
| `/dirs`                                                     | 仅返回实际隔离 cwd。                                                                                                                                                         |
| `/forget-approvals`                                         | 清理隔离 profile 的空授权记录，用户配置不变。                                                                                                                                |
| `/skillful status`                                          | 返回实际 skill listing 状态。                                                                                                                                                |
| `/extended-context status`                                  | 返回 off。                                                                                                                                                                   |
| `/computer status`                                          | 返回 disabled；没有访问显示器。                                                                                                                                              |
| `/ratchet`                                                  | 明确返回缺少 eval/task 工具；不算执行实验或推理。                                                                                                                            |

运行命令为 `python /tmp/ait-pr568-command-audit.py`。完整私有结果保存在
`target/provider-parity-validation/2026-11-06/official-command-execution.json`，
注册表审计在同目录 `official-command-matrix.json`；临时脚本和原生数据不提交。
没有在真实用户 profile 执行 logout、新建/删除会话、切换 worktree、对外分享或权限写入。
目录增删实测命令：`python /tmp/ait-pr568-command-audit.py --directories`；四步实际结果在同目录
`official-command-directory-execution.json`。原生工作目录及用户 profile 未改动。

### Test coverage

**not measured**：未请求覆盖率。上述 handler 执行数量不是测试覆盖率，也不是模型调用成功数；
如需测量，只限定 OMP 命令、侧问答和宿主时间线协调路径。

## 2026-11-06 至 07：真实模型与新增界面

Linux Electron、共享客户端和当前 Rust daemon，连接未经修改的官方 OMP 18.14.0。
使用隔离 profile 中已有可用渠道的真实 `futureppo/deepseek-v4.1-flash`，不使用 mock、
loopback 回复或合成侧答案。所有凭据及完整原生运行文件留在私有运行目录，不提交。

| 场景                | 实际结果与精选截图                                                                                                                                                                                                                                                                                                                         |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 真实模型与预设/角色 | `real_work` 的 default、vision、advisor 及 smol 绑定显示在真实会话设置；真实模型回答可见。[预设与角色](../../assets/providers/omp/real-model-roles-preset.png)、[真实回答](../../assets/providers/omp/real-model-response.png)。                                                                                                           |
| 主任务与侧问答并行  | 主任务仍为 running，侧面板收到真实流式回答；主时间线仅保留主回答，未混入侧 topic。[并行截图](../../assets/providers/omp/real-main-side-parallel.png)。                                                                                                                                                                                     |
| 侧问答取消          | 首个真实侧增量出现后取消，取消前后主任务仍为 running，turn ID 保持 `8da76ed6-acd3-4797-8dfc-07b256ba2cdb`。[取消截图](../../assets/providers/omp/real-side-cancel.png)。                                                                                                                                                                   |
| 原生历史恢复与追问  | 实际重启隔离 daemon 后，runtime/persistence native ID 均保持 `01a1b16f-35c6-7793-a2f7-4ad22e33ff79`；恢复原生 sidecar，`FOLLOWUP_AFTER_RESTART_568` 返回完整真实回答。[恢复后追问](../../assets/providers/omp/real-side-followup.png)。本步经 SDK 直接发送 `/btw`，当时正常 slash 入口暴露的目录 schema 失败单独记录，不冒充该入口已通过。 |

真实模型曾在侧回答中复述尚未完成的主上下文；原生 `get_btw_history` 包含同样文本，
Ait 不删除或重写模型内容。单次 `SIDE_SHORT_568` 的原生 sidecar 为 `status: complete`、
`answer: ""`，界面按原始记录显示完成但无正文，**该次不计模型回答成功**，不添加 fallback。

恢复曾失败的原因是测试 HOME 等于 cwd：官方 OMP 自动切到 `/tmp`，而 Ait 正确拒绝不匹配的
原生日志。所有 RPC 启动显式传入 `--cwd` 后，官方 HOME=cwd 两项定向检查通过；
真实模型后的重启执行与侧问答历史也已核对。早期只看缓存 native ID 的离线截图不算恢复成功。

### 预设表单与历史界面证据的范围

2026-11-06 早期隔离桌面检查还覆盖预设保存后草稿继续可编辑、新建 Agent 草稿成套应用和
Profile 同步模型/推理/角色。私有证据位于 `omp-roles-ui-X6ymTd/`：
`06-preset-saved-on-disk.png`、`09-draft-preset-complete-configuration.png`、
`10-profile-preset-atomic-selection.png`，另有保存/删除回执及 Profile 再打开截图。
这些操作由真实官方 OMP 执行，但使用无真实账户的测试模型配置；不把它们称为真实 LLM 验收，
也不使用原生恢复失败的早期截图证明重启成功。最终真实模型与角色证据在上一表单列。

结构化 Goal 的历史实际模型截图已人工查看并确认不含凭据，按原始日期引用，
不冒充当前工作树的全平台复验：

- [结构化 DSH Goal 完成卡](../../assets/providers/omp/historical-dsh-goal-card.png)：
  2026-10-31，Goal 状态、目标、轮数与操作按钮；原图显示实际 DeepSeek 模型会话。

### 最终正常入口、查询隔离与真实压缩

修复缺失 `subcommands` 被 Rust 可变索引写成 `null` 的实际 schema 错误后，Node SDK 命令目录
为 35 项、零 null，真实 App 的正常 slash `/btw` 成功打开历史；没有放宽 schema 或依赖绕过入口。
真实主任务的 `/usage` 前、完成后和 `/status` 拒绝后三次读取均为 running，
同一 turn `fb63271e-d4dc-4ac3-b67b-3744e9960cfb`；usage 卡返回 56,165 tokens。
`/status` 明确显示需要 TUI，原生 journal 没有该 user prompt，主任务继续执行。
见[正常入口查询与拒绝](../../assets/providers/omp/real-live-usage-status.png)。

独立真实模型会话经过正常用户输入及实际回答积累上下文后，`/compact` 实际成功：
**28,023 → 26,845 tokens，减少 1,178**。此前小会话的 Nothing to compact 与非法 focus
拒绝均不计成功；成功路径没有使用 mock 或修改原生模型回答。
见[真实压缩完成](../../assets/providers/omp/real-compact-complete.png)。

在已由真实模型产生持久记录的现有会话实际点击原生统计与 HTML 导出，均返回原生成功结果：
[真实会话统计](../../assets/providers/omp/real-native-statistics.png)、
[真实 HTML 导出](../../assets/providers/omp/real-native-html-export.png)。
没有为截图新增付费问答；最终评审图不再使用早期 loopback seed 图。
HTML 的原始 Electron 写盘/摘要证据仍按其历史范围引用，不把本次导出入口截图等同下载校验。

### 最终定向 Test execution

- **225 通过、0 失败、16 忽略**：OMP adapter、宿主时间线、直接输入调度及 AgentSession 端口。
  其中完整拒绝矩阵实际检查 61 个根命令、8 个别名及 11 个受限子命令均产生独立错误卡，
  原生请求日志字节不变；不是复制 allowlist 常量后检查相等。
- 官方 OMP 18.14.0 roles/preset 专项 **2 通过**，包括 HOME=cwd、原生角色/继承、持久恢复
  和限定名称的预设管理；测试使用本地协议桩形成模型日志，**不计真实 LLM 验收**。
- 同名 extension/custom 正常执行、inactive query 不创建主 turn、最终 ACK 后独立结果卡可见、
  重复 timeline.get 与 daemon/存储恢复不丢失或复制卡片均通过。
- 前端侧面板/原生 UI 组件 **6 通过**，Provider UI 协议 **3 通过**；本轮前端类型检查和
  四个相关文件 oxlint **0 警告、0 错误**。这些是修改前端后的既有定向结果，不冒充重新运行。
- daemon 构建、Rust fmt、Provider/API/daemon 全目标 Clippy `--no-deps` 通过；
  本地工具链沿用既有 `assert_is_empty` 例外。仅依赖 filesystem 的已有 `fetch_update` 弃用警告。
- 早期 **151 通过、2 失败、16 忽略** 是过时目录/argv 断言的失败记录，不作为最终通过；
  后续 voice 幂等回执排序回归及缺失字段 schema 错误亦已修复并纳入最终检查。
- 本地没有执行完整 Rust workspace 测试或覆盖率。

```sh
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target/provider-parity-build \
  cargo test --locked -q -p provider --lib -- \
  local::omp storage::timeline service::agent_execution::tests::delivery ports::agent_session
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target/provider-parity-build \
  AIT_TEST_OMP_BIN=/home/KirisameLonnet/Developers/ait/target/provider-parity-validation/2026-11-06/official-omp-18.14/omp-linux-x64 \
  cargo test --locked -q -p provider --lib -- --ignored local::omp::tests::installed::model_roles
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target/provider-parity-build \
  cargo clippy --locked -q -p provider -p api -p daemon --all-targets --no-deps -- \
  -D warnings -A clippy::assert_is_empty
```

本节源码为整合 `main@a8e8a24f` 的工作树，后续如只整合工作流变化，不重复上述 Rust 范围；
PR/CI 以最终提交为准。观测命令与结果摘要保存在 `/tmp/ait-pr568-final-backend-verification.log`，
不将摘要称为原始进程日志。覆盖率仍为 **not measured**：未请求，测试数量不代替覆盖率。
