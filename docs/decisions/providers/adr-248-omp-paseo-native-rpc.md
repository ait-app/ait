---
id: ADR-248
status: accepted
date: 2026-11-11
supersedes:
  - providers/adr-190-omp-acp-settlement.md
amends:
  - providers/adr-180-omp-native-rpc-provider.md
  - providers/adr-194-omp-settlement-reconciliation.md
  - providers/adr-196-provider-output-semantics.md
  - providers/adr-198-omp-interaction-progress.md
  - providers/adr-249-omp-adapter-crate.md
  - workspace/adr-215-sandboxed-worktree-isolation.md
  - workspace/adr-223-linux-worktree-sandbox.md
paths:
  - crates/provider-omp
  - crates/provider
  - bins/daemon
---

# ADR-248：OMP Provider 按 Paseo 设计直接驱动原生 RPC

- 状态说明：Accepted，取代 [ADR-190](adr-190-omp-acp-settlement.md)。
- 修订：[ADR-180](adr-180-omp-native-rpc-provider.md) 的启动、传输、取消、特性、用量与诊断；
  [ADR-194](adr-194-omp-settlement-reconciliation.md) 第 4 条；
  [ADR-196](adr-196-provider-output-semantics.md) 中 OMP `async-result` 的投影；
  [ADR-198](adr-198-omp-interaction-progress.md) 第 3 条；
  [ADR-249](adr-249-omp-adapter-crate.md) 的入口与公开项；
  [ADR-215](../workspace/adr-215-sandboxed-worktree-isolation.md) 与
  [ADR-223](../workspace/adr-223-linux-worktree-sandbox.md) 的 OMP 受保护路径（第 11 条）。

## 背景

ADR-190 让 Ait 自己的 OMP Provider 也经过进程内 Rust ACP 引擎（ACP→官方 RPC），并以
`_omp/rpc|event|respond` 透传原生控制；ADR-249 原计划再为外部 ACP 客户端提供独立的 `omp-acp`
二进制。所有者评审后认定这个设计方向错误：宿主路径多出一层 ACP 转换和 Ait 私有扩展，
行为与 Paseo 的 OMP Provider 越来越远，而 Ait 并不需要向外部 ACP 客户端提供 OMP。
所有者要求改用 Paseo 的设计。

Paseo（origin/main `a886bfbcd`）为每个会话启动一个 `omp --mode rpc-ui` 进程，用 OMP 原生
JSONL RPC 通信，把原生事件直接映射为会话事件。移植过程中逐项用已安装的官方 OMP 18.18.8
核对 wire 行为；Paseo 的假设与真实 OMP 不一致时，以真实 OMP 为准。

## 决策

1. **不使用 ACP。** 删除进程内 ACP 桥接引擎、`_omp/*` 扩展通道、`ait-acp-aliases/` 别名记录、
   `omp-acp` 二进制及其发布打包，以及 0.2.7 及更早版本的 `daemon omp-acp` 子命令，不提供替代入口。
   ADR-190 整体被取代。
2. **Paseo 式原生 Provider。** `OmpClient` 为每个会话持有一个未修改的 `omp --mode rpc-ui` 进程，
   经原生 JSONL RPC 收发，把原生事件直接映射为宿主回合事件、Timeline 条目和权限请求；
   模型目录与命令发现使用 `--no-session` 临时进程。行为以 Paseo 为基线，但保留 Ait 的以下约束：
   第 4 条的结算规则、`ask-cached` 档位、JSON 数组模型 ID（ADR-180 第 5 条）、持久句柄格式
   及其向后兼容（不写 `native_handle`）、Progress/Timeline 分离与确定性锚点键、`blob:sha256`
   图片解析、对话的 `cancel` 与期限、运行中缓存的 runtime 信息（ADR-198 第 1 条）、写入 confinement、
   现有数量上限，以及只为兼容旧配置而接受并忽略的 `transport` 特性键。
3. **修订 ADR-180。**
   - 启动参数采用 Paseo 的顺序：`--mode rpc-ui --approval-mode <档位>`，可选
     `--smol`/`--slow`/`--plan <模型>`、`--model <provider/id>`、`--thinking <级别>`、
     `--no-session` 或 `--session <会话文件>`、`--append-system-prompt <文本>`。未选择模式时为 `yolo`。
     首次恢复不传 `--model`，之后仅当所选模型在原生列表中时才切换；模式、系统提示或
     provider options 变化时，在回合之间以 `--session` 重启进程，并带上当前模型、推理级别和 Fast。
   - ready 后，若 OMP 宣告 v2 且帧尺寸为 1 MiB / 64 MiB，则协商 v2；否则静默留在 v1。
     物理行和重组帧的上限均为 64 MiB。无效 JSON、无 `type` 的帧和残缺分块只记录并丢弃，
     不再使会话失败。第 4 条的 1 MiB 输入帧限制与严格失败不再适用。
   - 取消改用 `abort`，取代 `abort_and_restore_queue`。OMP 保留排队的 steer 与 follow-up。
   - 特性只保留实时可切换的 Fast，删除 autoCompaction、autoRetry、slowMode 开关及服务档位探测进程；
     旧键仍被接受并忽略。压缩、交接、steer 与 follow-up 改为内置斜杠命令，在输入队列之外执行。
   - token/cost 与上下文用量改由 `get_session_stats` 读取；OMP 不支持该命令时，退回
     `get_state.contextUsage`。
   - 接受 Paseo 的 provider options：`smolModel`、`slowModel`、`planModel`、`rpcTimeoutMs`。
     其他键一律拒绝，包括 Paseo 的 `sessionDir`。
   - 诊断只运行 `omp --version` 并报告 OMP 的 profile 和目录，不启动会话进程。
   - `supportsDynamicModes` 为 true。
4. **保留 ADR-194 的结算规则，修订其第 4 条。** 整轮仍只在收到关联的 `prompt_result` 加
   `session_settled` 后，或经 `get_state.isSettled` 核对后完成；不采用 Paseo 的
   `agent_end` 加空闲判定。取消新增一种确认来源：`abort` 确认之后到达的 `agent_start`
   说明 OMP 已为排队的输入开始新的运行。此时被取消的回合以 Cancelled 结束，新的运行成为自主回合，
   排队的追加输入保留各自的客户端消息 ID。运行时尚未看到该自主回合时，
   `AgentSession::pending_foreground` 返回 true，运行时不会再把新输入作为 prompt 提交进去。
   五秒的确认期限不变。
5. **修订 ADR-198 第 3 条：采用 Paseo 按标题判定的工具审批。** 满足以下两个条件的 `select`
   映射为带 shell、edit 或 write 详情的工具权限：标题以 `Allow tool: bash|edit|write` 开头；
   选项由 `Deny`、一次性批准（`Approve` 或 `Approve Once`）以及可选的 `Approve …`/`Always Approve …`
   范围组成。OMP 的会话、目录和文件范围保留为额外的允许动作。其他 `select` 仍是通用问答。
   `confirm` 改为 Yes/No 问题，回答为 `{confirmed}`。对 `ask` 工具，Ait 把每个选择字段显示为
   一张问题卡；用户作答后，由回放状态机自动回应 OMP 的后续 `select`、`input` 和 `editor`，
   不再询问用户。ADR-198 的其余条款不变。
6. **修订 ADR-196 中 OMP 后台结果的投影。** OMP 的 `async-result` 不再投影为 Background job
   工具结果，改按 Paseo 的自定义消息规则：`<system-notice>` 正文投影为通知，`<task-result>`
   显示为 “Background job <id> <status>” 并按状态定级；其他可见自定义消息投影为以 `customType`
   命名的文本工具行。旧 Timeline 中的后台结果卡在下次历史校验时经 generation 替换。
   ADR-196 的其余条款不变。
7. **修订 ADR-249。** 不提供 `omp-acp` 二进制。`provider-omp` 只公开 `OmpClient` 和
   `installed_program`。`provider` 只公开 OMP 生产代码实际使用的项（当前清单见 ADR-249 第 3 条）：
   `local::{prompt_blocks, tool_detail}` 与 `acp_transport::response` 恢复为私有。
   `ImageStore::source` 保持 `local` 内可见（第 10 条后已无外部使用者）。
8. **暂缓。** 以下各项暂不实现：宿主工具与 MCP 桥接（`set_host_tools`、`host_tool_*`）、
   经 `branch` 的会话回退、迟到 steer 拒绝后的重新排队（P2：现有 inbox 不会自动重放已认领的写入）、
   基于 `available_commands_update` 的会话级命令缓存（P4：需要新增会话级端口）。
   在实现之前，`host_tool_call` 与 `host_uri_request` 使当前回合失败；`supportsMcpServers`
   与 `supportsRewindConversation` 保持 false；迟到的 steer 拒绝显示为警告通知，回合继续。
9. **最低版本为 OMP 18.12.6，而不是 Paseo 的 16.3.9**：Ait 依赖该版本的 RPC v2 和 `isSettled`
   判定（ADR-194）。诊断按此判定版本是否受支持。Paseo 中成本较低的兼容回退照样移植。
10. **文本模型的图片交给 OMP 处理（2026-11-12 所有者决定）。** 无论当前模型的 `input` 是否含
    `image`，prompt 与 steer 的图片都随请求发送给 OMP；没有图片时仍省略 `images` 键。不再按 Paseo
    把图片写入 daemon 私有目录并以 `[Image available at: …]` 代替。OMP 18.18.8 实测：它把图片保存为
    会话目录下的 `local://image-<hash>.webp`，在用户消息之前追加一条 `display: false`、
    `customType: image-attachment-description` 的自定义消息（每张图片一个文本块，说明没有配置视觉模型），
    用户消息保留图片，发给模型的只有文字和 `[image omitted: …]`。该隐藏消息在实时与历史中都不显示，
    沿用“`display: false` 不显示”的规则。
11. **沙箱中保护 OMP 的配置与代码（修订 ADR-215、ADR-223）。** 受限的 OMP 进程仍可写配置根目录
    （`~/.omp` 或 `$HOME/$PI_CONFIG_DIR`）、`PI_CODING_AGENT_DIR` 指定的 agent 目录和
    `$XDG_DATA_HOME/omp`，以下条目保持只读，因为之后不受限的 OMP 进程（包括发现与命令探测）会遵从或执行它们：
    - agent 目录（默认 `<配置根目录>/agent`，及 `PI_CODING_AGENT_DIR`）中的 `config.yml`、`config.yaml`、
      旧版 `settings.json`、`models.yml`、`models.yaml`、旧版 `models.json`、`mcp.json`、`.mcp.json`、
      `.env`，以及 `extensions`、`hooks`、`tools`、`plugins` 目录；
    - 配置根目录的 `.env`、`plugins`、`natives`（OMP 解出并加载的原生 addon）、`python-env`；
      已存在的 XDG data 根目录的 `natives`、`plugins`、`python-env`；
    - 默认 profile 的会话还保护配置根目录与 XDG data 根目录下的整个 `profiles`。

    会话 JSONL、`blobs`、`agent.db`（含认证刷新）、其他数据库、日志、缓存与 `run` 仍可写。
    缺少 `config.yml` 时 OMP 会把 `settings.json` 迁移进去；受限会话中这次写入被拒绝，OMP 只记录警告并
    照常运行，迁移留给下一次不受限的启动。新版本的 `natives/<版本>` 也只能由不受限进程（例如模型发现）
    解出；在此之前受限启动以 addon 加载失败退出。
    Linux 为这些缺失条目初始化占位：目录为空目录，`.yml` 与其他 YAML/JSON 一样写入 `{}`（OMP 拒绝空的
    `models.yml`），并先创建缺失的父目录。`StateLocation` 只能表达静态路径，命名 profile 自身的
    `profiles/<名称>` 配置无法单独保护，命名 profile 的会话因此保持其可写。

## 真实 OMP 与 Paseo 的差异

下表只列出影响决策的差异，均以 OMP 18.18.8 实测为准。

| 主题             | Paseo 的假设                                    | OMP 18.18.8 实测                                                      | Ait 采用                                                    |
| ---------------- | ----------------------------------------------- | --------------------------------------------------------------------- | ----------------------------------------------------------- |
| 恢复时模型不可用 | OMP 回退到默认模型                              | 会话记录的模型无法解析时，rpc-ui 恢复以退出码 1 失败                  | 与 Paseo 一样首次恢复不传 `--model`；这种失败按启动失败报告 |
| 工具审批选项     | 恰好 `Approve`、`Deny`                          | 还有 `Approve <tool> for this session` 及目录/文件授权选项            | 接受这些选项，作为额外的允许动作                            |
| `ask` 工具       | `questions` 参数，自定义文本由 `editor` 收集    | 参数为 `{i, html}` 表单，逐字段提问；选择 Other 后由 `input` 收集文本 | 按实测回放                                                  |
| `abort`          | 写入 `abort` 后在本地结束回合                   | 保留排队输入，确认后立即为它们开始新的运行                            | 第 4 条                                                     |
| 本地命令         | 确认返回 `agentInvoked: false` 后直接结束回合   | 此后既无 `prompt_result` 也无 `session_settled`                       | 立即核对原生空闲状态后收尾，仍遵守第 4 条                   |
| 会话目录         | 读取 `settings.json` 的 `sessionDir`            | 忽略该设置（文件被迁移到 `config.yml`）                               | 不读取；拒绝 `sessionDir` 选项                              |
| Fast 被拒绝      | 抛出错误                                        | 不支持 Fast 的模型拒绝 `set_fast_mode`                                | 记录日志，保留 OMP 当前的值                                 |
| 子 Agent ID      | 使用原始任务名                                  | 子会话文件按任务名存放在各自父会话的目录下，不同会话可有同名任务      | 使用 `<父会话 ID>/<任务名>`，避免同一 cwd 中合并时冲突      |
| 文本模型收到图片 | 写入本地文件，以 `[Image available at: …]` 提示 | OMP 已自行保存图片，并附加隐藏的说明消息                              | 交给 OMP 处理（第 10 条）                                   |

## 后果

- OMP 只剩宿主内嵌的一条路径，没有外部 ACP 入口；旧的 `daemon omp-acp` 用户没有替代方案。
- 旧句柄中的 `transport: acp`、`nativeTransport: rpc` 以及旧特性键仍能加载，但值被忽略；
  原生会话文件和 Ait 句柄格式不变，已有 Agent 可以直接恢复。
- 工具审批重新依赖 OMP 的标题格式。OMP 改动标题或选项时，审批退化为通用问答，不会被误判为授权。
- 文本模型拿不到 Ait 写出的图片路径；需要分析图片时，由用户在 OMP 中配置视觉模型角色（`modelRoles.vision`）。
- 受限 Agent 不能改写 OMP 的设置、模型、MCP、扩展与原生 addon。仍可写的提示类内容（`AGENTS.md`、
  `SYSTEM.md`、skills、commands、rules、prompts、agents、memories）与 ADR-215 已知缺口中的 Claude
  hooks、Codex skills 同类。Linux 上首次受限启动初始化的 `config.yml` 或 `models.yml` 会遮住只存在于
  `config.yaml`、`models.yaml` 或未迁移的 `models.json` 中的配置，这些用户需先改用 `.yml`。
  macOS Seatbelt 只按路径拒绝写入：把受保护条目的可写祖先目录改名、修改后再改回，可以绕过保护；
  这同样适用于 ADR-215 的其他受保护路径，Linux 的祖先挂载点不受影响。
- 当前行为规则见 [Oh My Pi 现行规格](../../architecture/omp.md)，配置与限制见
  [运维文档](../../operations/oh-my-pi.md)。离线 Python 夹具按 OMP 18.18.8 的 wire 格式维护；
  可选的安装版测试（`AIT_TEST_OMP_BIN`）用真实 OMP 和本地模拟模型验证，不调用付费模型。
