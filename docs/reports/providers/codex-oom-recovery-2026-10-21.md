# Codex OOM 后恢复与错误上报

- 日期：2026-10-21；平台：Linux x86_64；Rust 1.99.0。
- 源码基线：`ecb3705d` 加本次恢复改动；实际提交见 PR 最新修订。
- 决策：[ADR-177](../../decisions/providers/adr-177-codex-restart-continuation.md)。

## 故障证据

系统日志显示，2026-10-20 22:51:38（Asia/Shanghai），systemd-oomd 选择
`ait-daemon.service` 并杀死相关进程。22:51:39 服务记录 `oom-kill`，22:51:42 自动重启。
原生会话停在未完成工具调用、轮次保持 inProgress；旧安装包的持久记录仍为 running。
现场运行 daemon 0.1.5，不能据源码版本断言已经部署了后续恢复修复。

## 最终行为

重启恢复先通过持久队列接回原 Codex 会话。已有原生执行或完成事件优先；需要新轮次时，
发送独立的继续指令，核实未完成工具操作，不重新发送原始提示词。已有排队用户工作优先。
接纳结果不确定的恢复指令不重发，无法恢复的记录进入 error。
恢复指令不改写最后用户输入时间，确保原生已写入、回执尚未提交时再次中断也不会绕过去重。

原生接纳失败仍使用脱敏的实际原因。修复两处错误丢失：投递层不再用泛化的排队错误
覆盖 lastError；finish.wait.error 不再始终返回 Provider execution failed。
失败说明在聊天、持久状态与等待结果保持一致，之后仍能在同一原生会话中显式继续。

恢复指令被领取后、原生连接尚未完成时，claimed 仍视为待接纳的前台工作，避免等待接口
把这一时窗误判为执行失败。连接成功前保留持久 running，失败时再提交终态。

没有修改宿主 cgroup、OOM 策略或其他模型服务；没有部署或重启正在服务其他任务的 daemon。

## 验证

定向测试覆盖：重复入队幂等、排队用户工作优先、uncertain 不重发、恢复连接期间的 running
持久状态、接回原生运行／完成事件、继续成功、原生拒绝、provider 不可用及失败后的重试。
接回的原生失败同样直接上报，不再自动开启新轮次覆盖失败。

真实 daemon 进程测试使用离线 Codex stdio peer，SIGKILL 宿主及原生 writer，随后启动同一
数据目录，验证自动继续与失败上报两条路径。没有人为耗尽内存，也没有把模拟 SIGKILL
描述成重新触发真实 OOM。

执行命令与范围（默认 features，仅 Linux）：

恢复、投递、等待及接纳屏障的定向回归共 **95 项通过**，真实 daemon 强杀重启用例
**2 项通过**。测试结果与下文覆盖率测量状态分开报告。

```sh
cargo test --locked -p provider --lib service::agent_manager::tests::recovery
cargo test --locked -p provider --lib service::agent_execution::tests::recovery
cargo test --locked -p daemon --test process codex_oom_restart
```

为避免等待另一个构建任务的 Cargo 锁，以下相关过滤器直接复用本次编译的 provider 测试程序：

```sh
target/debug/deps/provider-90d8f10180c6fa37 claimed_foreground_input_keeps_completion_pending_until_admission_or_recovery
target/debug/deps/provider-90d8f10180c6fa37 service::agent_execution::tests::delivery
target/debug/deps/provider-90d8f10180c6fa37 service::agent_execution::tests::waits
target/debug/deps/provider-90d8f10180c6fa37 service::agent_manager::tests::resume
target/debug/deps/provider-90d8f10180c6fa37 service::agent_manager::tests
```

最后一个过滤器验证了更新后的文本输入测试适配器及其 80 项 manager 回归；与先前执行的
恢复和 resume 过滤器重复的用例只计数一次，其余 provider 用例仍未执行。

真实进程测试先暂停宿主，再 SIGKILL 原生 writer 和宿主，避免关闭 stdio 后原生进程自行退出
使注入故障的测试产生竞争；恢复过程仍由真实 daemon 在原数据目录中完成。

provider 严格 all-targets Clippy 受到既有 Pi／恢复元数据等测试的 `assert_is_empty` 检查阻挡。
本次新增的一处相同检查已修正；仅在检查命令中放行该既有 lint 后通过：

```sh
cargo clippy --locked -p provider --all-targets -- -D warnings -A clippy::assert_is_empty
```

daemon 严格 Clippy 受到未修改的 `schedule::engine` 的 `single_element_loop` 和
`filesystem::workspace_runtime` 的 `fetch_update` 弃用检查阻挡；跳过依赖后还发现既有进程
测试中的 `assert_is_empty`。对 daemon 目标跳过依赖并放行同一条既有测试 lint，检查本次
进程测试。没有修改这些无关实现。

```sh
cargo clippy --locked -p daemon --test process --no-deps -- -D warnings -A clippy::assert_is_empty
```

## PR 更新复查

恢复改动提交为 `19a829a4`；与主线 `a73c16ba` 合并后的代码版本为 `5e05fb43`。
合并冲突仅涉及两处文档索引，双方条目均保留，本次 Rust 恢复实现没有变化。
在该版本上重新验证了 **95 项 provider 测试和 2 项 daemon 强杀重启测试，全部通过**：

```sh
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target cargo test --locked -p provider --lib service::agent_manager::tests
/home/KirisameLonnet/Developers/ait/target/debug/deps/provider-e83349c4251c8663 --quiet service::agent_execution::tests::recovery service::agent_execution::tests::delivery service::agent_execution::tests::waits claimed_foreground_input_keeps_completion_pending_until_admission_or_recovery
CARGO_TARGET_DIR=/home/KirisameLonnet/Developers/ait/target cargo test --locked -p daemon --test process codex_oom_restart
```

`cargo fmt --all --check` 通过，文档本地链接检查通过（426 个 Markdown 文件），Codex
Python 测试进程的语法检查通过。未部署或重启正在服务用户任务的 daemon。

CI 的 Rust 1.98.1 检出了两处测试入口持有 17,080 字节异步状态的 `large_futures` 问题。
测试入口改用 `Box::pin` 承载该状态，恢复实现与测试步骤不变；两项强杀重启测试、上述
daemon 定向 Clippy 和 `cargo fmt --all --check` 再次通过。

## Test coverage

**not measured**：用户未请求覆盖率测量；测试通过数量不是行覆盖率。没有覆盖率产物。
后续若请求测量，使用 cargo llvm-cov 限定上述 provider 模块及 daemon Codex 进程用例。
本次未手动运行完整 Rust workspace 测试；macOS、Windows 的重启恢复未实测。
