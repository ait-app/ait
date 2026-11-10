# PR #612：运行时与配置复审

评审基线：`d3a2223252d8688fa6f3358e1c682dbf728a2b0a`。本报告对应 necokeine 的 10 条行内意见。
前 8 条有代码路径依据；后 2 条依赖原生 OMP 行为，单独验证，不混记为已确认缺陷。
本轮只修复本 PR 的行为，不执行已推迟到下一 PR 的 ACP 拆分，也不修改 UI 或补拍截图。

## 确认问题与修复

| #   | 问题                                                                        | 修复与验收                                                                                                                                                                              |
| --- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 重建 OMP 进程后旧 action 回执仍存活，可能误收重用的请求 ID                  | 关闭旧 transport 前失败并清空 action 回执；回归检查回执立即结束、相同 ID 不再被旧 action 捕获、新 prompt 能完成。                                                                       |
| 2   | Vibe 启用只提取能力通知，丢弃其他帧                                         | 只消费自身能力通知，按原顺序恢复 BTW、action、插件对话框等帧；能力确认失败的回滚同样恢复事件。                                                                                          |
| 3   | 第 9 个并发 Vibe 工具调用使整个 foreground turn 失败                        | 为该调用返回 `host_tool_result` 错误，现有 8 个调用继续；拒绝回执使用有界批次和读取背压，避免错误路径无界创建任务。                                                                     |
| 4   | 32 条历史记录变成会话终身创建上限                                           | 只淘汰最旧的未使用终态记录，并回收空闲 writer。运行中、启动中、send/kill/wait 正在使用的记录受保护；worker 在确认追问前提交运行状态，迟到投影不能重新插入已淘汰记录。完整原生日志保留。 |
| 5   | 取得 completion 后 poll 失败，唯一负责结算的 future 被丢弃                  | 仍返回持有 durable receipt 的 completion；故障注入验证持久化 poll 失败后回执可结算，相同 message ID 重试不会重复发送命令。                                                              |
| 6   | 文本查询尚未结束时可以提交 builtin，输出被归入查询卡                        | 在任何配置/目标变更和 native prompt 写入前返回 Busy；查询结束后允许 builtin，查询输出仍留在原卡片。                                                                                     |
| 7   | 部分配置事件把完整运行快照写入持久配置                                      | 原生事件携带三态 patch：缺省保留、null 清除、值替换；跨内部序列化后语义不变。thinking 更新不固定继承的模型，model 更新不清除未提及的 thinking。                                         |
| 8   | Claude advisor 目录探测失败导致全部 draft features 失败，空目录反复启动 CLI | CLI 可用时保留静态 features；失败及空结果按 cwd 缓存 30 秒，最多 32 个缓存项，同步并发探测；过期后可恢复探测。                                                                          |

代码入口：

- [Session 准入与重启](../../../crates/provider/src/local/omp/session.rs)、[原生配置事件](../../../crates/provider/src/local/omp/session/observed_settings.rs)。
- [Vibe host](../../../crates/provider/src/local/omp/vibe/host.rs)、[记录淘汰与操作保护](../../../crates/provider/src/local/omp/vibe/host/registry.rs)、[worker 状态提交](../../../crates/provider/src/local/omp/vibe/host/worker.rs)。
- [命令回执](../../../crates/provider/src/service/agent_manager/delivery.rs)、[配置持久化](../../../crates/provider/src/service/agent_manager/streaming.rs)。
- [Claude draft 探测](../../../crates/provider/src/local/claude.rs)。

## 两条依赖原生行为的意见

**9. 完成 Goal 后保留对象导致不能再次进入。** 官方 OMP 18.17.0 的完成路径会清空 Goal 模式状态，并追加 `goal-completed` 日志。见[上游实现](https://github.com/can1357/oh-my-pi/blob/v18.17.0/packages/coding-agent/src/modes/rpc/rpc-goal.ts)。本轮实装测试通过真实原生 `goal` 工具完成目标，核对原生日志和空状态，再次创建 Goal、进入及退出 Vibe。该版本未复现所述问题，因此没有按猜测放宽活动 Goal 的互斥条件；这不代表其他 OMP 版本已验收。

**10. 两个 BTW 并发使单个 running ID 被覆盖。** 官方 18.17.0 在原生入口拒绝第二个正在启动/运行的 BTW，见[上游实现](https://github.com/can1357/oh-my-pi/blob/v18.17.0/packages/coding-agent/src/modes/rpc/rpc-btw.ts)。实装检查保持第一个本地模型请求未完成，尝试第二个问题，核对只保留第一个运行话题并可取消。Ait 也补充显式 Busy：原生命令与 GUI action 均不接受重叠问题，历史查看和取消仍可用；已完成话题仍支持 Follow up。保留单 running ID，未虚构原生并发能力。

## 验证

环境：Linux x64、Rust 1.98.1、用户安装的官方 `/usr/bin/omp` 18.17.0。所有外部实装检查使用隔离配置及回环模型协议夹具，不访问付费模型，也不宣称真实上游推理或推理质量。

所有命令在本报告所在提交的修复内容上执行；本地 Rust 工具链通过独立 PATH 固定为 1.98.1。

| 命令（共同前缀 `cargo test --locked -p provider --lib`）                                               | 结果                                                              |
| ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------- |
| `local::omp`                                                                                           | 220 通过，26 忽略                                                 |
| `local::claude`                                                                                        | 85 通过，2 忽略                                                   |
| `service::agent_manager`                                                                               | 152 通过，1 忽略                                                  |
| `protocol::agent_config`                                                                               | 3 通过                                                            |
| `local::omp::tests::installed::vibe -- --ignored`，设置 `AIT_TEST_OMP_BIN=/usr/bin/omp`                | 6 通过，含连续 33 个 worker、审批、取消、追问、恢复和模型档位隔离 |
| `local::omp::tests::installed::review -- --ignored`，同一外部 OMP                                      | 2 通过：Goal 完成日志与再次进入；BTW 重叠拒绝及取消               |
| `installed_omp_btw_follow_up_and_branch_preserve_native_history_and_reopen -- --ignored`，同一外部 OMP | 1 通过：Follow up、Branch、父日志保留与重启续聊                   |

反向验证：临时移除前 8 项对应的关键修复，8 个独立探针均产生预期测试失败，随后恢复生产修复并重新通过上述定向检查。这是故障探针验证，不是覆盖率，也不是对所有历史版本的实装验收。
最终复核另补一个外部恢复断言：恢复调用返回前，worker 必须已登记为 running。该断言在保留旧终态记录时失败；注册阶段现直接更新状态，避免恢复回执与后续事件之间的淘汰空窗。随后重新验证 OMP 模块及 6 项 Vibe 实装检查。
探针分别位于 [Session review](../../../crates/provider/src/local/omp/session/tests/review.rs)、[Vibe host](../../../crates/provider/src/local/omp/vibe/host/tests.rs)、[记录容量](../../../crates/provider/src/local/omp/vibe/host/tests/registry.rs)、[Manager review](../../../crates/provider/src/service/agent_manager/tests/review.rs)、[部分配置事件](../../../crates/provider/src/local/omp/session/observed_settings/tests.rs)、[Claude draft](../../../crates/provider/src/local/claude/tests/draft.rs)。

`cargo fmt --all --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo build --locked --workspace`、`npm run check:format`、`npm run check:docs` 全部通过。仅运行受影响模块及直接关联测试；本地未运行完整 Rust workspace 测试。UI 没有变化，未重复使用旧截图宣称本次并发修复已通过 GUI 验收。最新提交的 CI 结果单独记录于 PR。

## Test coverage

**未测量**：用户未请求覆盖率，测试通过数量不代表覆盖率。若后续需要，使用 `cargo llvm-cov` 对本次涉及的 OMP、Claude draft、配置事件与命令回执定向测量，并记录提交、covered/total、忽略范围和可分享报告；不自动扩展为完整 workspace 测试。
