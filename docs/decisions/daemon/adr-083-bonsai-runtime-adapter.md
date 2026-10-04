# ADR-083：Bonsai 执行端适配器

- 状态：Proposed
- 日期：2026-10-04（2026-10-05 移植到 ADR-072 的目录与包名）
- 延续：ADR-047（只依赖 `model`、经 port 请求宿主的独立 crate）、ADR-072（名称与依赖守卫）
- 对照：ADR-074（账户主机中继）
- 用户要求：Bonsai 看板上的任务可以派给这台机器上的 Claude / Codex 会话；封装要薄，官方 harness 的设定在 Bonsai 里都调得到；挂到空间之后，空间成员都能用这台机器。

## 背景

Bonsai 定义了执行端协议 v1（会话契约 `bonsai.session/1`）：执行端出站连到用户自己的 Bonsai，接收 `run.dispatch`，在本机起 agent 会话，把会话翻译成中立事件发回去；网页上的空间成员能看、回话、打断、批权限、停止。协议原文（`docs/runtime-protocol.md`）和给 AIT 的适配说明（`docs/runtime-ait-adapter.md`）在 Bonsai 的服务端仓库里，该仓库不公开；本 ADR 按其 runtime 分支 `43ced8e`（2026-10-04）实现，适配器依赖的要点摘在下面「协议要点」一节，代码注释里的「协议 §n」「适配说明 §n」指这两份文档的小节。

bonsai-server 的 v2 设计（`docs/superpowers/specs/2026-10-01-bonsai-runtime-design.md` 第 8 节与附录 B）里有几条假设被证伪，本 ADR 取代它们：接入点不是 `AgentClient` / `AgentSession` 端口，而是进程内的 `AgentExecution::execute`；timeline 事件没有稳定 id，线上只有 `(epoch, seq)`，轮次和权限事件没有 seq、不落库；会话不原样透传 `ait/v1`，而是翻译成中立契约；`config.toml` 是 `deny_unknown_fields`，凭据只走环境变量；普通会话原来没有办法只保留注入的 MCP；信任单位是空间，成员可以选不经审批运行；执行端不读看板，任务内容随 dispatch 来。

## 协议要点

- 传输（§2）：`wss://<host>/runtime`，请求头 `Authorization: Bearer <token>`；一帧是一条文本消息：单行 JSON head，`session.events` 另带 body（事件数组）。文本 `ping` / `pong` 心跳每 25 秒，60 秒没有 `pong` 断开重连；不认识的帧种类忽略。
- 上限（§2）：`session.events` 整帧 256 KiB，hello 的 head 128 KiB，`run.status` 的 head 16 KiB，其余 head 8 KiB；单个事件 64 KiB、`tool.input` / `tool.output` / `ask.detail` 各 16 KiB、`final_text` 4 KiB，都由执行端截断。Hub 只查 head，超限关 `4400`。
- 限速（§2，按连接）：实时帧持续 20 帧/秒、突发 100；回答订阅的帧 200 帧/秒、突发 2000；`run.status` 2 帧/秒、突发 20，Hub 自己 `run.query` / `run.cancel` 问出来的状态不计；超限关 `4429`。
- 关闭码与退避（§2）：`4400` 帧不合法（丢掉那一帧、退避重连），`4401` 凭据被撤销（不重连，停掉所有 run），`4409` 被同一 `runtime_id` 的新连接顶掉（不重连），`4408` / `4429` / 网络错误退避重连；退避 1 秒起翻倍、封顶 60 秒、带抖动，收到 `runtime.welcome` 才清零；握手 HTTP 401 等同 `4401`，429 / 503 退避（503 照 `Retry-After`）。
- 握手（§3）：首帧 `runtime.hello`，每条连接只发一次，内容变了就断开重连；hello 不含本机路径，声明 provider、模型、`approvals`、`bonsai_write`、设定清单（每个 provider 至多 32 项）和项目（`git_remote` 归一成 `host/path`）。`runtime.welcome` 带机器主人。
- run（§4）：状态 `queued < claimed < running <` 终态（`completed` / `failed` / `cancelled`），只进不退；执行端先落库再报 `claimed`，重复的 dispatch 只回状态，取消过的 `run_id` 不再起；`failed` 带六种 `reason_code` 之一；连上之后由 Hub 逐个 `run.query` 对账，执行端不主动补发。dispatch 带 `bonsai.mcp_url`：执行端只有能把写回限定到它时才报 `bonsai_write: true`（§4.2、§8-7）。
- 会话（§5、§6）：每个事件带 `(epoch, seq)`，订阅按游标重放、一次答完并以 `sync` 结尾；成员的 `session.send` 按 `input_id` 幂等、不打断进行中的轮次，`session.interrupt` 结束当前一轮，`session.answer` 只认这个 `ask` 发出去的选项、第二次回答忽略。事件种类：`input`、`input_rejected`、`text`、`reasoning`、`tool`、`todo`、`ask`、`ask_resolved`、`turn`、`notice`、`usage`、`closed`；翻译不了的降级成 `notice`，图片降级成一行文字。

## 决策

1. 新建 `bonsai`。它只出站连接用户配置的那一个 Bonsai（`wss://<host>/runtime`，回环地址可用 `ws://`），说 Bonsai Runtime 协议 v1。不开入站端口，不做 E2EE、二维码或中继；和 `relay`（ADR-074，经账户服务的中继把客户端连到这台主机）不同，它不经第三方，信任域只有用户自己的 Bonsai。
2. 只在设置了 `BONSAI_RUNTIME_URL`、`BONSAI_RUNTIME_ID`、`BONSAI_RUNTIME_TOKEN` 三个环境变量时组装；只设了一部分时启动失败，报错只点名变量、不回显值。`BONSAI_RUNTIME_URL` 是 origin，适配器换算成 `ws(s)://<host>/runtime`；带路径、query、fragment、userinfo 的拒绝，`http://` 只允许回环主机。token 以 `SecretString` 持有，只在构造握手请求头时取出，不进日志、`Debug`、错误文案或数据库；WebSocket 库在 trace 级别会打印原始握手请求，所以 daemon 的日志过滤把 `tungstenite` / `tokio_tungstenite` 封顶在 debug。
3. daemon 起的每一个子进程（provider、终端、workspace 脚本、git / gh / glab / ssh、后台 git fetch、语音）启动时都剥掉 `BONSAI_RUNTIME_*` 与 `AIT_SERVER_*`（前缀不分大小写，Windows 的变量名不分大小写）：变量名由 `model::process::private_environment()` 统一给出，在显式配置的环境之前剥，所以显式配置仍然生效。`bins/daemon/tests/child_environment.rs` 钉住全部进程构造点，新增构造点不登记就失败。
4. 对内仅依赖 `model`，通过 Executor / Observer / Backfill / Projects 四个 port 请求宿主：执行走 `AgentExecution::execute`，事件走 `Timeline::events()` 按 agentId 观察，补帧走 `Timeline::read`，项目走 `Directory`（`list_projects`、`open_workspace`）。依赖边界新增 `bonsai -> model`、`daemon -> bonsai`（[当前架构](../../architecture/README.md)的能力边界表与 `bins/daemon/tests/dependencies.rs` 同步）。不新增客户端可见的 RPC，不改 Paseo 导入的文件。适配器跑在自己的线程和 current-thread runtime 上，daemon 的 `Server` 持有句柄，在 `Api::wait_closed()`（provider worker 关闭）之前停下。
5. Bonsai 的 `run_id` 是持久身份：本地 SQLite（`<data_dir>/bonsai/runtime.sqlite3`，Unix 上目录 0700、文件 0600）先记 run 再建 Agent；Agent ID 与 `idempotencyKey` 由 `run_id` 派生，重复派发只回状态，取消过的 `run_id` 记墓碑、不再起。AIT 重启后，适配器在连接之前用派生的 Agent ID `agent.get` 每个未结束的 run，只接回带着这个 run 的 `bonsai.run` 标签的 Agent（快照读不出来就核对不了标签，同样不接、也不碰那个 Agent）：接回会话、用 `Timeline::read` 补行，上一轮被重启打断的补 `turn{aborted, runtime_restarted}`；接不回的把挂着的请求撤回、排队的输入拒掉，报 `failed(session_lost)`。上一次 welcome 里的机器主人也记在库里，重启后到下一次连接之前主人在 AIT 里打的字照样记在主人名下。
6. 会话被翻译成中立事件，适配器自行编号 `(epoch, seq)`、先落盘再发，可按游标重放；轮次与权限事件同样编号。150 毫秒内的同一消息文本、相邻思考、同一工具的快照先合并再编号。每个事件在落盘前压进 64 KiB（依次减半最长的文字、去掉尾部的条目，最后换成一条错误提示），状态帧压进 16 KiB，hello 超过 128 KiB 时从尾部去掉项目；没有一个合法的输入能让一帧被 Hub 拒绝。实时帧按连接排队，所有 run 合起来约 15 帧/秒，状态与回答的桶同样留四分之一余量（1.5/秒、突发 15；150/秒、突发 1500：Hub 对这两种第一帧超限就关连接）；订阅回答原子地发完，答到冲掉实时帧之后的日志末尾；Hub 问出来的状态（`run.query` / `run.cancel`）不计入状态的限速，其余状态排队等桶，不堵住别的帧；心跳由发送任务在帧与帧之间发，长回答期间也照常。Bonsai 以 `4400` 拒绝某一帧时，那一帧（按帧记下各自的 seq 范围）换成一条错误提示并换 epoch，不会在重连后原样再发。输入队列归适配器：远端输入不打断正在进行的轮次。
7. 看板内容（任务行、所在小节、派发者补充）只进第一个用户消息，转义（`&` 先转）后放进带标签的块；system prompt 只追加固定前言、Bonsai 的收尾指令（仅在写回受限于 dispatch 的 MCP 地址时）与派发者填写、执行端声明为 system prompt 的 `append_system_prompt`。权限选项只暴露一次性或本会话有效的，不暴露持久写入用户设置的选项（Claude 的 `userSettings` / `projectSettings` / `localSettings` 建议、Codex 的前缀与网络规则）；Codex 的 `grantRoot` 请求只给本会话的授权和拒绝。原生请求 id 不合契约形状的映射成稳定的哈希 id，回答时用原生 id。
8. 设定透传：执行端在 hello 里按 AIT 的白名单声明 Claude / Codex 的设定（权限模式来自 `provider.modes.list`），dispatch 时逐项做最后校验；`execution.approvals` 按几项设定合成之后的实际策略算，并拒绝「选了标为不经审批运行的项、合成之后仍会停下来问人」的组合；主人中途改模式时，按新模式加上这次 run 自己的 `approval_policy` / `sandbox_mode` 重算并如实提示。会扩大「需要审批」含义的目录设定（`add_dirs`、`writable_roots`）不声明；Codex 0.156 拒绝加载 `approval_policy = "untrusted"`，所以它不在选项里。OpenCode 与 DeepSeek Harness 不在 hello 里声明：它们没有 strict MCP（第 10 条），派到它们的 run 做不到「会话里的 Bonsai 授权只覆盖本空间」。
9. 信任单位是 Bonsai 空间：挂载后，空间成员都能派发、观看、回复、批准，并可选择执行端声明的任何设定（含不经审批运行）；适配器不以派发者身份降级或拒绝，但如实报告是否不经审批运行。
10. 会话一律以 strict 模式启动，只加载本会话自己的 MCP 服务器：Claude 加 `--strict-mcp-config`；Codex 在建线程前带着工作目录读有效配置（`config/read`，含项目层），把不属于本会话的 MCP 服务器、插件逐个关掉，再整体关掉插件与 ChatGPT apps；用户配置里已有同名的服务器时拒绝建会话（Codex 会把用户的头、令牌或命令合并进注入的那一个）。为此 `provider` 的 `providerOptions` 新增 `strictMcp`（Claude、Codex 各自实现）。主人自己的 Bonsai plugin / 连接器通常拿着他所有空间的授权，挂上的空间的成员不能借它碰到别的空间，所以从不复用（协议 §4.2、§8-7）。Bonsai 在本机（回环）时注入名为 `bonsai_run` 的服务器指向 dispatch 的地址，报 `bonsai_write: true` 并追加收尾指令；远端 Bonsai 的会话里没有任何 Bonsai 工具，报 `bonsai_write: false`、不追加收尾指令。
11. 取消只在确认执行已停下之后报告终态；Codex 30 秒内未确认则归档并结束进程组。空闲 15 分钟后归档、报 `completed`（最后一轮失败则 `failed(provider_error)`）。主人在 AIT 里改模式或归档，AIT 不推事件，适配器以所有 run 合计每秒至多 2 次的 `agent.get` 轮询发现并如实提示；轮询只预约名额、不阻塞会话。观察断开期间丢掉的轮次结束与权限请求没有 timeline 行，适配器按快照对账：快照里有、本地没有的请求立刻补上；本地开着、快照里已经没有的轮次或请求，连续两次轮询之间没有任何事件才结束或撤回，刚开始的轮次不会被误结束；取快照时已在通道里的事件先处理，被快照结束的那一轮（按 AIT 的 `turnId`）之后迟到的结束事件忽略。观察打不开时按退避重试，不让会话卡死。会话结束前后到达的回话一律回 `input` 或 `input_rejected`，取消一律回状态；会话停止收命令之后才到的，等它结束再由协调器从库里回答，一个 run 的日志始终只有一个写者。

## 后果

- 新增 `crates/bonsai`；`bins/daemon` 的组装（`host/bonsai_runtime.rs`）、配置读取、日志过滤与依赖边界表；各子进程启动处的环境剥离；`provider` 的 `strictMcp`（Claude 参数、Codex 线程覆盖）。顺带修正 Claude Code 2.1.x 会在 stdout 回显权限回答（`control_response`）而 AIT 把它当协议错误、结束会话的问题：只放过刚回答过的那个原生请求的回显。
- 三个 `BONSAI_RUNTIME_*` 都不设时，daemon 的行为与此前相同（子进程环境少了这两类前缀的变量）。删除该 crate 只失去「被 Bonsai 派活」这一能力。
- 剥离只管继承：显式配置的环境、shell 启动文件里的 export、以及能读同用户进程环境的子进程，都不在它的范围内（见验证报告的残留风险）。
- 验证见 [Bonsai 执行端适配器验证](../../reports/daemon/bonsai-runtime.md)。

## 以后再议

- 远端 Bonsai 的写回：给每个挂着的空间一条只对那个空间同意过的专用连接，在 strict 会话里注入，并核对它的地址等于 `mcp_url` 之后才报 `bonsai_write: true`（协议 §4.2；AIT 里怎么按空间挑连接未定）。
- OpenCode 与 DeepSeek Harness 的 strict MCP：实现之后再在 hello 里声明。
- Codex 不确认打断时只有取消有 30 秒宽限；打断本身的宽限要先定「轮次在本地结束而 Codex 仍在跑」时后续输入怎么办。
- token 是否也可从 0600 的文件读入：需与 `docs/policy/rust.md` 的环境变量规定一并决定。
