# Bonsai 执行端适配器验证

AIT 作为 Bonsai Runtime 协议 v1 的执行端：出站连到用户自己的 Bonsai，接 `run.dispatch`，
在本机起 Claude / Codex 会话，把会话翻译成中立契约 `bonsai.session/1` 发回去；空间成员能看、回话、
打断、批权限、停止。决策与协议要点见 [ADR-083](../../decisions/daemon/adr-083-bonsai-runtime-adapter.md)，
用法见 [daemon 手册](../../operations/daemon.md)的「Bonsai 执行端」一节。
协议原文在 Bonsai 服务端仓库（不公开）runtime 分支 `43ced8e`（2026-10-04），实现与验收都按这一版对照。

源码提交：`8ba34e70`（剥离凭据）、`cff05dad`（provider strict MCP）、`eb12b645`（适配器），
基于 upstream main `3a13d02b`；之后只有文档提交。平台 macOS `aarch64-apple-darwin`；Rust 1.98.1
（`rust-toolchain.toml`）；cargo-llvm-cov 0.9.1；Claude Code 2.1.289；codex-cli 0.156.1。

## 范围

- 新 crate `crates/bonsai`（只依赖 `model`）：配置、线上帧、中立事件、hello、连接与退避、
  单一发送任务、run 协调、会话任务（含重启恢复）、设定声明与校验、提示词、SQLite 存储。
- `bins/daemon`：只在三个 `BONSAI_RUNTIME_*` 都设了时组装（`host/bonsai_runtime.rs`）；
  日志过滤把 WebSocket 库封顶在 debug；依赖边界表。
- 所有子进程启动处剥掉 `BONSAI_RUNTIME_*` / `AIT_SERVER_*`（`model::process`），并有清点测试：
  18 个生产文件、37 个进程构造点，其中三处是本次基线新增的：后台 `git fetch`、DeepSeek Harness 的 ACP
  进程、OpenCode 的 `serve` 与 `--version` 探测。
- `provider`：`providerOptions.strictMcp`（Claude `--strict-mcp-config`；Codex 按工作目录 `config/read`
  后关掉继承的 MCP 服务器、插件与 apps）；Claude Code 2.1.x 回显权限回答（`control_response`）不再结束会话。

## 移植到当前 main

这批改动最初写在 `d69567f` 上（五个提交，其中一个是 clippy 1.96 的 lint 修正）。移植到 `3a13d02b` 时：

- 目录与包名按 ADR-072：`bins/server` → `bins/daemon`，`server-*` → 无前缀；新 crate 叫 `bonsai`，
  依赖一律走 `[workspace.dependencies]`（新增 `bonsai`、`rustls` 两项）。daemon 里组装它的模块改名
  `host/bonsai_runtime.rs`：叫 `bonsai` 会遮住同名的 crate。
- 去掉 lint 修正那一个提交：工具链固定在 1.98.1，main 上已经干净；新代码按 1.98.1 的 clippy 与
  main 新加的 lint（`unused_crate_dependencies`、`unused_qualifications` 等）修过。
- 上游这段时间的行为变化，适配器跟着改，并各有回归测试：
  - Codex 的 MCP 调用行改成 `name: "<server>.<tool>"`、`detail.type: unknown`（0.0.16，#167），原来只认
    `mcp__…` 和 `mcpToolCall`，Codex 调 `bonsai_run` 的工具会显示成普通工具，参数里的笔记路径还被当成
    本机路径去掉（`translate::codex_mcp_rows_named_server_dot_tool_become_mcp_tools`）；
  - Codex 新增的 `error` 时间线条目（「Selected model is at capacity…」）原来显示成「显示不了的记录」，
    现在是 `notice{error}`（`translate::provider_error_rows_become_error_notices`）；
  - Codex 思考力度新增 `max` / `ultra`，设定声明跟上；
  - 适配器每分钟一次的「项目 / provider 有没有变」检查：provider 目录缓存 60 秒，每次问可用性都会把所有
    已装的 provider 重新探测一遍（Claude、Codex，以及新加的 OpenCode `serve`、DeepSeek ACP），改成项目每分钟查、
    provider 每 15 分钟查（重连时照旧全量）（`link::offer_checks_ask_for_providers_only_every_fifteen_minutes`）。
- 移植后起了一轮四个视角的审查（移植是否走样、provider 侧语义漂移、daemon 生命周期漂移、仓库规范），
  24 条发现逐条做反驳验证，确认 7 条并已修正：上面四条中的三条（provider 检查频率、Codex `error`、
  ADR 依赖不公开的仓库 → 补了「协议要点」），另外是提交拆分、缺失的报告、`strictMcp` 的判断写了两份。
  四个回归测试都做过反向修改（恢复原样后对应测试失败）。

## 审查与修正（初版）

初版实现后起了三路独立审查（会话语义、协议、安全），逐条核对代码之后修正并补回归测试；修完之后又起了四路对抗验证
（会话、协议、安全、测试质量），再修一轮。两轮合起来：

- 协议：超限事件、状态帧、hello 一律压进上限，存量的超限事件发送时换成同 `seq` 的错误提示；
  Hub 问出来的状态不计限速、其余状态排队不堵别的帧；心跳由发送任务在帧与帧之间发；
  `4400` 按帧记各自的 seq 范围；pong 超时用独立的截止时刻；项目 / provider 变化的检查不阻塞入站帧；
  空 id 与不合形状的原生请求 id 映射成合法 id；`run_id` 形状按 `r_` + 32 位十六进制。
- 会话：观察断开期间丢掉的轮次结束与权限请求按快照对账（两次安静的轮询才结束 / 撤回）；
  会话收尾时到达的回话和取消照样回答；轮询只预约名额不阻塞会话；主人在 AIT 里打字开新一轮；
  观察打不开时退避重试；输入在 AIT 收下之后才记为已发；请求事件和它的记录一起落盘；
  机器主人记在库里，重启后照样归属；接不回的 run 撤回请求、拒掉排队输入；
  主人改模式时连同 run 自己的 codex 覆盖项重算 `approvals`；重启恢复只接带本 run 标签的 Agent。
- 安全：删掉 `BONSAI_RUNTIME_MCP=reuse`（协议 2026-10-04 `8875ceb` 起要求会话里的 Bonsai 授权只覆盖本空间，
  做不到就不给 Bonsai 工具）；WebSocket 库在 trace 级别会打印含 `Authorization` 的原始握手请求，日志过滤封顶在 debug；
  环境变量前缀不分大小写；数据库目录 0700、文件 0600；未知类型的工具详情去掉像本机路径的字段；
  Codex 的 `config/read` 带工作目录，项目层的 MCP 服务器也会被关掉；Codex strict 再整体关掉插件，
  用户配置里有和注入的服务器同名的就拒绝建会话（Codex 会把用户的头、令牌或命令合并进去）；重启恢复读不到快照就不接；
  Codex 的 grantRoot 请求在选项和详情里写明要放开的目录。
- 第二轮另修：快照对账按 AIT 的 `turnId` 忽略被结束的那一轮迟到的结束事件，取快照时已到的事件先处理；
  观察断开期间主人打的字照样开轮次；会话停止收命令之后才到的命令等它结束再由协调器回答（一个 run 的日志只有一个写者）；
  连接结束时等发送任务最多 5 秒；项目变化要去抖之后再核对一次才重连，且不改共享的 offer；订阅回答在冲掉实时帧之后才读日志末尾；
  状态与回答的桶留余量；存储的暂时性错误不答 `no_history`；有更新的状态在排队时，被问出来的旧状态不顶掉它。
- 端到端里查出：Codex 0.156 拒绝加载 `approval_policy = "untrusted"`（「is no longer supported; remove this setting」，
  而它的协议 schema 里还有这个值），带这个设定的派发都在建会话时失败。设定里不再声明它。

## 协议清单（适配说明 §2.3）

单元测试在 `crates/bonsai/src/**/tests.rs`（下表以「模块::测试名」引用），端到端见下一节。另有一个进程级测试
`bins/daemon/tests/process/bonsai_runtime.rs`：真 daemon 二进制以 trace 日志连到测试里的假 Hub，核对握手路径、Bearer 头、
hello 不含本机路径、派发经真 `AgentExecution` 和 Claude 夹具走到 `claimed` / `running` / `completed`、事件连续编号、日志里没有 token。

| # | 条目 | 证据 |
| ---: | --- | --- |
| 1 | 只出站，不开入站端口 | crate 里只有 `connect_async`；`service::the_worker_dials_the_endpoint_and_shutdown_abandons_a_stalled_handshake`；E2E |
| 2 | 非回环只走 wss；Bearer；token 不进 URL / 日志 | `config::*`（9 条）、`service::debug_output_names_the_service_without_credentials`、`daemon` 的 `the_websocket_library_never_logs_its_handshake_at_trace`、`bonsai_runtime_is_off_unless_configured_and_never_printed` |
| 3 | 子进程环境里没有 `BONSAI_RUNTIME_*` | `bins/daemon/tests/child_environment.rs`（构造点清点）、`process::child_environment::native_agents_never_inherit_server_or_runtime_credentials`（真 Claude 包装进程）、`model` 的 `process::*` |
| 4 | 心跳 25 秒，60 秒无 pong 断开 | `link::heartbeat_pings_and_a_silent_hub_is_dropped_and_redialled`、`outbox::the_heartbeat_goes_out_between_the_frames_of_a_long_answer` |
| 5 | 不认识的 type 忽略 | `wire::heartbeat_and_unknown_frames_decode_without_errors`、`runs::welcome_and_unknown_frames_are_ignored` |
| 6 | 关闭码与握手 | `link::close_4401_*`、`close_4409_*`、`handshake_401_*`、`handshake_429_*`、`handshake_503_*`、`close_4400_*`（共 37 条 link 测试）；E2E 撤销 |
| 7 | 各项上限 | `event::*`（含 `every_kind_with_unbounded_text_is_shrunk_to_fit`、`an_event_that_cannot_shrink_becomes_an_error_notice_with_its_seq`）、`wire::oversized_status_texts_are_halved_until_the_head_fits`、`wire::an_oversized_stored_event_goes_out_as_a_notice_with_its_seq`、`hello::an_oversized_hello_leaves_out_projects_until_it_fits` |
| 8 | 限速 | `outbox::concurrent_streams_share_one_live_budget_without_losing_events`、`status_frames_pass_through_their_own_bucket`、`statuses_the_hub_asked_for_are_not_counted`、`waiting_statuses_never_hold_up_other_frames`、`runs::queries_and_cancels_credit_their_run_before_the_status` |
| 9 | hello 首帧、welcome 前不发、内容变了重连 | `link::nothing_but_the_hello_is_sent_before_welcome`、`hello_is_the_first_frame_and_carries_no_local_paths_or_secrets`、`changed_projects_reconnect_with_a_new_hello_after_the_debounce`、`offer_checks_ask_for_providers_only_every_fifteen_minutes` |
| 10 | hello 内容 | `hello::*`（8 条，含 `remotes_normalize_like_bonsai` 对照 Bonsai 的 `normalizeRemote`）；E2E：Hub 存下的 hello |
| 11 | 设定声明与最后校验 | `settings::*`（67 条，含穷举 codex 组合的 `codex_every_combination_agrees_with_bonsai_unattended_of`）；E2E 设定透传 |
| 12 | 先落库、重复只回状态、墓碑 | `runs::dispatch_records_the_run_and_reports_claimed_before_any_agent_call`、`duplicate_dispatch_*`、`cancel_of_unknown_run_buries_it_*`、`dispatch_of_tombstoned_run_*` |
| 13 | provider / model 为 null | `runs::dispatch_starts_one_agent_and_reports_running_with_the_resolved_model`、`session::the_model_ait_reports_becomes_the_execution_model` |
| 14 | 每帧状态带 execution，reason_code 六种 | `runs::query_of_known_run_*`、`invalid_dispatch_is_recorded_then_fails_without_execution`、`wire::status_heads_omit_absent_fields` |
| 15 | running 显式；空闲关会话 | `session::idle_session_closes_after_the_grace_period_and_completes`、`idle_close_after_a_failed_turn_reports_provider_error`、`the_owner_typing_in_ait_opens_a_turn_the_idle_timer_respects`；E2E |
| 16 | 不主动补发状态 | `runs::run_serves_inputs_until_shutdown`、`outbox::events_logged_while_disconnected_are_not_resent_live` |
| 17 | 不可信内容只进 user turn | `prompt::*`（9 条）、`session::system_prompt_never_carries_task_fields` |
| 18 | 写入口只有 mcp_url | `runs::bonsai_write_is_true_only_when_a_loopback_mcp_url_is_injected`、`write_policy_*`、`session::create_*strict*`、`provider` 的 `claude_strict_mcp_*` / `codex_strict_mcp_*`、`translate::codex_mcp_rows_named_server_dot_tool_become_mcp_tools`；E2E 工具名 |
| 19 | 取消 | `session::cancel_*`（5 条）、`runs::cancel_*`（6 条）、`recover::a_cancel_recorded_before_the_restart_ends_the_run_cancelled`；E2E 停止 |
| 20 | 日志、游标重放、换 epoch | `store::*`、`outbox::epoch_rotation_*`、`session::timeline_replacement_*`、`quarantine_*`、`recover::a_new_ait_generation_after_a_restart_keeps_the_log_and_moves_the_cursor` |
| 21 | 原子的订阅回答 | `outbox::answers_flush_live_first_and_mark_reset_and_sync`、`answers_continue_after_a_cursor_or_restart_on_a_stale_one`、`wire::replay_frames_are_consecutive_and_bounded`；E2E 重订 |
| 22 | 没有记录的 run | `outbox::unknown_runs_get_no_history_and_unavailable_refs_go_out`、`runs::send_and_answer_for_an_unknown_run_report_unavailable_with_their_ref`、`interrupt_without_a_live_session_sends_nothing` |
| 23 | 所有事件都有 seq | `session::dispatch_input_and_turn_start_are_logged_before_create_returns`、`turn_end_withdraws_pending_asks_before_the_turn_event` |
| 24 | send 不打断、幂等、input / input_rejected | `session::input_during_a_turn_*`、`a_repeated_input_id_is_ignored`、`refused_delivery_*`、`input_that_arrives_while_the_session_closes_is_still_answered`、`runs::send_to_a_finished_run_*`；E2E 排队 |
| 25 | interrupt | `session::interrupt_*`、`queued_input_becomes_the_next_turn_after_an_interrupt`；E2E 打断 |
| 26 | answer 只认发出去的选项，第二次忽略 | `session::a_second_answer_to_a_resolved_ask_is_ignored`、`an_option_the_ask_never_offered_*`、`translate::deny_and_allow_responses_echo_only_native_actions`；E2E 重复回答被忽略 |
| 27 | ask 的形状与选项 | `translate::tool_requests_offer_only_one_time_or_session_grants`、`codex_drops_persistent_grants_and_grant_root_requires_session_scope`、`request_ids_outside_the_contract_pattern_are_remapped_and_answered_natively`、`session::requests_only_the_snapshot_shows_*` |
| 28 | 主人改模式发 notice | `session::owner_mode_change_seen_in_the_snapshot_warns_once_and_is_recorded`、`an_owner_mode_change_keeps_the_runs_own_codex_overrides` |
| 29 | approvals 如实 | `settings::*unattended*`、`runs::claimed_execution_reports_approvals_as_the_settings_resolve`；E2E `approvals false` |
| 30 | 图片降级 | `translate::images_never_expose_local_paths_or_remote_urls` |
| 31 | 流式文本先合并 | `buffer::*`、`session::assistant_deltas_coalesce_into_one_text_event_per_mid_after_the_window` |

## 端到端

只对本机：Bonsai 服务端 runtime 分支 `43ced8e` 的 `git archive` 快照里跑 `scripts/runtime-sandbox.mjs`
（隔离的 wrangler dev、临时 D1 / R2、DEV_GRANT 只给沙盒空间），AIT 用临时数据目录、端口 7427，二进制是
`eb12b645` 编出来的 `target/debug/daemon`，Claude / Codex 用本机 CLI。网页上的动作由脚本按 run 页的表单和
WebSocket 帧发出，空间成员 `sandbox-member` 派发、回话、批权限；另有一个看门狗订阅每个 run，出现任何不是注入的
`bonsai_run` 服务器的 MCP 工具就立即杀掉 AIT（全程没有触发）。AIT 进程带一个不监听的 `HTTPS_PROXY`，
`NO_PROXY` 只放行 localhost、OpenAI / Anthropic 与 npm registry（本机 Claude Code 有一个经 npx 的 SessionStart
hook，挡住 npm 会让 AIT 的 Claude 探测超过 30 秒而报不可用），即使 strict 失效也连不到别的 Bonsai。

| 场景 | 结果 |
| --- | --- |
| 连接与 hello | Hub 存下 claude 12 个模型（默认 provider）、codex 7 个；项目 remote 归一为 `github.com/freezind/bonsai-sandbox-test`；codex 的思考力度带 `max` / `ultra` |
| 派发 → 流式 → 批权限 → 重启（Claude Sonnet 5.5，`r_64b7…`） | 成员派发；有挂着的请求时 SIGTERM 重启 AIT：同一 epoch 接上，`ask_resolved{withdrawn}`、`turn{aborted, runtime_restarted}`；成员回话开新一轮，五个请求由成员批准（`ask_resolved` 带 `by` / `login`），写好 `/tmp` 的文件，经 `mcp__bonsai_run__read_note` / `set_task_state` / `append_to_note` 写回沙盒笔记并勾掉任务 |
| 回话不打断、打断、停止（Claude Haiku 4.5，`r_612a…`） | 流式输出中的回话立刻有 `input`，轮次不受影响；打断 → `turn{aborted, interrupted}`，排队的回话成为下一轮；下一轮中停止：挂着的请求 `withdrawn`、`turn{aborted}`、`closed{cancelled}`，状态 `running → cancelled` |
| 设定透传 | Claude `permission_mode=bypassPermissions` → `approvals: false`（`r_4865…`）；Codex `sandbox_mode=read-only` + `effort=low` + `append_system_prompt` → `approvals: true`，每条回复最后一行是追加的 `settings-ok`（`r_283a…`） |
| Codex（`r_283a…`） | 只走 `bonsai_run.list_spaces` / `read_note` / `set_task_state` / `append_to_note`（Bonsai 上是 `kind: mcp`、标题 `bonsai_run · read_note`）写回并勾掉任务；成员在会话里要求写 `/tmp` 的文件 → 只读沙箱申请提权，成员批准后写入。派发者补充里的同一要求，Codex 当成看板数据、不作为授权（提示词把看板内容放进不可信的块，符合预期） |
| 重启后接回（Codex，`r_283a…`） | 空闲时重启 AIT：真实快照带 `bonsai.run` 标签，同一 epoch 续号，成员回话开新一轮，Codex 线程照常 resume |
| 空闲关闭 | Codex `r_283a…` 空闲 900 秒后 `closed{idle}`、`completed`；Claude `r_4865…` 最后一轮失败（Anthropic 的安全分类器拦下了 Sonnet 5.5 的输出，与适配器无关）之后空闲 900 秒 `closed{idle}`、`failed(provider_error)`「最后一轮失败之后没有新的输入」 |
| 撤销（Claude Haiku 4.5，`r_5ed7…`） | 流式中在账号页撤销：Hub 先发 `run.cancel`（本地记下取消请求），再关 `4401`；会话以 `turn{aborted, revoked}`、`closed{revoked}` 结束并归档；适配器记「halted; not reconnecting reason=Revoked」，100 秒内没有任何重连，AIT 照常响应；Hub 上 run 为 `failed(runtime_revoked)` |
| 日志不含 token | AIT 以 debug 和 trace 各跑一段：runtime token、AIT token、`Authorization` 都是 0 次；trace 下 WebSocket 库只有 debug 的一行握手 |
| 工具前缀与事件检查 | 5 个 run 的全部 MCP 工具调用：Claude 只有 `mcp__bonsai_run__*`，Codex 只有 `bonsai_run.*`；沙盒 `[check] ✗`（网页读不懂、超限、seq 不连续）与 `[bonsai]`（Hub 记的错）都是 0 条，四个关闭的会话分别查过 27 / 36 / 39 / 68 个事件 |

## 测试执行

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test --workspace --no-fail-fast` | 2,176 passed、0 failed、3 ignored（main 原有的 3 项需要真 CLI 或真模型请求） |
| `cargo test -p bonsai` | 373 passed |
| 中间提交 | `8ba34e70`：fmt、clippy、清点守卫通过，`model` / `provider` / `filesystem` / `metadata` / `terminal` / `voice` 共 1,476 passed；`cff05dad`：fmt、clippy、清点守卫通过，`provider` 591 passed |
| 关键回归的变异检查 | Codex `server.tool` 行、`error` 行、provider 检查间隔、前缀大小写不敏感各做一次反向修改，对应测试都失败 |
| `node --test scripts/check-docs.test.mjs`、`node scripts/check-docs.mjs` | 通过 |

全量运行里有一次 `directory_sync::websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints`
在 10 秒的接收超时上失败（机器上同时在跑别的负载）；单独重跑三次与整个 `process` 目标重跑都通过，上表是之后的完整运行。

## Test coverage

测量版本：`eb12b645`（Rust 源码；逐文件 SHA-256 记录在[覆盖率 JSON](bonsai-runtime-coverage.json)），之后只提交了文档。
范围是整个 Cargo workspace、默认 features、macOS `aarch64-apple-darwin`；rustc 1.98.1、cargo-llvm-cov 0.9.1，
工具默认的文件过滤，没有自定义排除，不插桩 doctest。为了省磁盘，覆盖率构建设了
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`（不影响插桩）。

```sh
cargo llvm-cov --workspace --html
cargo llvm-cov report --json --summary-only --output-path <scratch>/cov-summary.json
cargo llvm-cov report --lcov --output-path <scratch>/cov.lcov
```

| 范围 | 行覆盖率 | 已覆盖 / 总行数 |
| --- | ---: | ---: |
| Rust workspace | 94.37% | 54,855 / 58,130 |
| bonsai（含 `testing.rs` 等测试辅助） | 94.14% | 5,434 / 5,772 |
| bonsai 生产代码 | 93.93% | 5,231 / 5,569 |
| daemon | 94.82% | 1,079 / 1,138 |
| provider | 93.78% | 21,490 / 22,915 |
| model | 94.99% | 758 / 798 |
| filesystem | 94.93% | 11,156 / 11,752 |

覆盖率运行为 2,175 passed、0 failed、3 ignored（比普通运行少 1 项，覆盖率构建下有一项测试不跑）。和 main 上最近一份
同平台的 workspace 测量（[Cargo workspace 依赖整理](cargo-workspace-pr-coverage-2026-10-04.json)，`7c1f6eba`：94.50%，
49,024 / 51,879）相比低 0.13 个百分点；那次带 `--test-threads=1`，main 之后又有提交，`3a13d02b` 本身没有测，所以这只是
历史对照。几个关键文件：`session.rs` 91.21%、`session/recover.rs` 91.62%、`link.rs` 91.12%、`outbox.rs` 88.69%、
`event.rs` 84.43%、`translate.rs` 94.46%、宿主一侧的 `host/bonsai_runtime.rs` 91.08%、`model::process` 100%、
`provider` 的 `configuration.rs` 95.67%。JSON 里有每个改动的生产文件的行数和未覆盖行（LCOV DA，和 LLVM 摘要的分母不混算）；
HTML 在本地 `target/llvm-cov/html/index.html`。

主要缺口：SQLite / 进程的失败注入（存储写失败、AIT 进程起不来）、`event::fit` 的部分极端分支、Windows 上的大小写变量名。
高行覆盖率不代表这些都验过。

## 残留风险与未做

- 远端 Bonsai 没有写回（`bonsai_write: false`），按空间授权的专用连接见 ADR-083「以后再议」。
- OpenCode、DeepSeek Harness 没有 strict MCP，不在 hello 里声明；它们的子进程环境在代码里剥离并登记在清点表，
  没有像 Claude 那样的进程级行为测试。
- 子进程构造点的清点只核对文件里有剥离调用，不核对每一处都经过它（同一文件里的第二个构造点漏了也查不出）。
- Windows 上 portable-pty 会把注册表里的环境变量并进终端的子进程；剥离只按 daemon 自己的环境列名字，
  只存在注册表里的同名变量剥不掉（main 原有的 `AIT_SERVER_*` 剥离同样如此）。
- Claude Code 自己插入的用户消息（例如安全分类器截断之后的提示）在 AIT 里是一条用户消息，适配器按「主人在 AIT 里打的字」
  显示成机器主人的输入。
- 撤销之后适配器什么都不再报，本地库里那条 run 仍记为 `running`；Hub 以 `failed(runtime_revoked)` 收尾。
- Codex 不确认打断时轮次一直开着；成员可以停止（30 秒后归档并结束进程组）。
- 真正的 timeline 改写（不是重启）只保留输入与挂着的请求，此前合成的轮次标记和已结束的请求从新 epoch 里消失。
- `4400` 记的是最后写出的那一帧；Hub 拒绝的若是更早一帧，隔离的范围可能不对（日志里可见）。
- 预先放行的列表放行了几乎所有工具时仍报 `approvals: true`（适配说明 §4.5 第 4 条的取舍）。
- Codex 项目层的 MCP 服务器靠 `config/read` 的 `cwd` 参数（schema 说明），没有在受信任的项目里实测。
- AIT 自己的 Codex 选项白名单（`provider` 的 `configuration/options.rs`）仍收 `approval_policy: "untrusted"`，Codex 0.156 会拒绝；
  这里只在适配器的声明里去掉了，没有改 AIT 的白名单。
- 机器上全权限的 agent 读得到 daemon 自己的环境变量（同一用户的进程环境可读；协议已写明：移出成员后撤销重配）。
- 开放中的 PR #169 新增了 DeepSeek 原生宿主的进程构造点，并且 ADR 编号与本 PR 可能相撞：后合并的一方要把新构造点登记进
  `bins/daemon/tests/child_environment.rs` 并调用 `model::process::private_environment()`，必要时改 ADR 编号。
