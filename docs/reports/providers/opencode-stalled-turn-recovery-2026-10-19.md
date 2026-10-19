# OpenCode 输出停滞与宿主重启恢复

- 日期：2026-10-19。
- 源码基线：`c517756a`；修复对应本报告所在提交。
- 平台：Linux；真实 daemon 进程与隔离的离线 OpenCode ACP 夹具。
- 决策：[ADR-032](../../decisions/providers/adr-032-daemon-native-provider-execution.md)、
  [ADR-144](../../decisions/providers/adr-144-provider-error-reporting.md)。

本报告记录恢复子集；合并 Codex 和客户端错误诊断后的最新验证见
[PR 整合报告](../daemon/error-diagnostics-and-recovery-pr-2026-10-19.md)。

## 截图会话的根因

只读核对 gpu-desktop 的 systemd journal、daemon 进程启动时间和原生会话记录。
截图会话使用 OpenCode 1.18.4；时间统一为北京时间：

| 时间        | 证据                                                                                                  |
| ----------- | ----------------------------------------------------------------------------------------------------- |
| 14:06:24–25 | 原生 assistant 输出“61 个测试通过，继续跑完整后端与 student-profile 测试”，随后启动 bash/Maven 工具。 |
| 14:08:01    | `systemd-oomd` 因系统内存与 swap 使用均超过 90%，将 `ait-daemon.service` cgroup 标记为杀死。          |
| 14:08:04    | 当前 daemon 的启动时间，确认此前执行者已经退出并自动重启。                                            |
| 14:38:00    | 用户在停住后手动中断并继续，新输入启动新的原生轮次。                                                  |

因此，这次停滞有明确的外部进程终止原因；后续 cancel/Aborted 不能作为原始停顿原因。
OOM journal 中的内存数字表示系统压力，不能据此断言 daemon 自身泄漏了这些内存。
本次没有修改远端配置、取消会话、重启服务、重发模型输入或部署代码。

## 源码缺口与修复

`AgentExecution::spawn` 此前只恢复过期审批、输入 receipt 与标题，未收尾持久的 `running`。
重启后没有 live writer，却继续返回 `status=running`；`activeTurn` 缺失时客户端兼容逻辑
还会根据 running 状态推导活动轮次，因此一直显示转圈。用户中断后可以继续，不能证明
原来的执行者仍然存在。

修复留在现有 Provider 协调层：执行 worker 接纳请求之前，收尾无 live writer 的遗留
running，先保存固定重启提示，再提交错误状态与 attention。持久化失败阻止启动；稳定的
宿主诊断标识使部分成功后的重试保持幂等。原生历史重新加载仍保留诊断。
原生会话句柄、模型设置、用量及其他 metadata 保留，显式 Continue 仍走 ACP 原生恢复。
不会自动重发已执行输入，也没有新增静默超时、自动 Continue 或第二条控制通道。

这项修复解决重启后虚假运行的状态，不防止系统 OOM。OpenCode 子进程单独退出的现有
失败收尾另有真实进程回归覆盖；持续静默但进程仍活着的原生模型/工具请求不因此被强制取消。
2.x 的既有 ACP 复查结果见 [独立报告](opencode-2x-acp-2026-10-19.md)。

## 验证

修复前的真实 daemon 强制退出／重启回归失败：首个快照仍返回 running，没有 live turn。
新增回归在原生 ACP 已接纳输入后再杀进程，避免只验证尚未开始的输入。

在独立工作树复核本次 PR，使用默认 features，保持测试并行度：

- Agent 执行／状态／目录回归：208 passed，包括 5 项新增恢复单测。
- OpenCode 离线回归：83 passed、27 ignored；需要真实安装版的用例本轮未运行。
- daemon OpenCode 真实进程回归：5 passed，包括 daemon 与原生 ACP 子进程的 SIGKILL，
  恢复后首个快照、wait 错误终态、原生身份保留、显式 Continue 和历史重读。
- Provider 全 targets 与 daemon 生产二进制的严格 Clippy 通过。
  daemon process target 的严格 Clippy 仍被未改动测试中的 18 项
  `clippy::assert_is_empty` 阻挡；新增 OpenCode 进程测试没有 lint 错误。
- 初次 OpenCode 并行回归暴露 HTTP 夹具先释放保留 socket 再绑定的竞争。
  夹具已移到 HTTP 模块的子测试文件，直接移交保留 socket；修复后完整的 OpenCode
  离线过滤器通过，没有降低测试并行度。

```sh
cargo test --locked -p provider --lib service::agent_ --quiet
cargo test --locked -p provider --lib local::opencode --quiet
cargo test --locked -p daemon --test process unix::opencode --quiet
cargo clippy --locked -p provider --all-targets --no-deps -- -D warnings
cargo clippy --locked -p daemon --bin daemon --no-deps -- -D warnings
# 已执行，受上述基线测试 lint 阻挡
cargo clippy --locked -p daemon --test process --no-deps -- -D warnings
cargo build --locked --workspace
cargo fmt --all --check
node scripts/check-docs.mjs
```

`cargo build --locked --workspace` 通过；未修改的 filesystem 存在 `fetch_update` 弃用警告。
Rust 格式、5 份 Markdown 格式和 390 份文档的本地链接检查通过；Python 夹具语法检查通过，
未生成仓库内字节码或提交运行数据。远端未部署，未调用真实模型；其他平台未验证。

## Test coverage

**not measured**：用户未请求覆盖率统计；通过用例数不代表覆盖率，没有生成覆盖率产物。
如后续请求，使用 `cargo llvm-cov -p provider --lib service::agent_manager::tests::recovery --html`
测量恢复模块的定向测试；真实 daemon 故障注入另使用 daemon process 集成测试过滤器。
未运行全 workspace 测试或 workspace 覆盖率；其他平台未验证。
