# 性能交接报告修复验证（2026-11-03）

## 范围与版本

- 输入：`ait-performance-handoff-20261103.zip` 中的报告和已有证据。私有采集、生产数据库、
  会话标识未导入仓库；没有启动生产采集、重启 Ait 或重放用户 prompt。
- 基线：`git fetch origin main` 后的 `ef942807787feff9fdb2209c84ccfcd9f56ce505`
  加本次修复修改，Linux x86_64，仓库锁定的 Rust 1.98.1。最终验证均以此基线为准。
- 原工作区的两个旧提交完整保存在本地分支
  `backup/innocent-racoon-before-main-20261103`；本轮改动从远程 main 重新适配。

## 修复行为

1. OMP 原生 mapper 将 `proc://` Read/Edit/Write 映射为保留原始 input/output 的通用工具，
   不作为文件内容写入。共享 mapper 仅复制符合 optional string/number 契约的参数，保留空字符串。
2. 现有历史兼容入口同时覆盖 canonical 重建、缓存反序列化和 source fallback：
   省略 `write.content:null`，修正旧进程资源展示；沿用主线 shell null cwd/output 的兼容。
   不删除条目、不改 canonical、epoch、seq 或 source coverage，不放宽客户端 schema。
   修复后的进程包装大小计入旧缓存字节预算，超预算时按 source 行分页。
3. registry 的 `snapshot()` / `get_shared()` 共享已提交记录。成功写入后失效视图，失败和
   no-op 保持旧视图；拥有记录的 get/list 在释放锁后复制。目录匹配、分页、Workspace activity
   和 attention scan、原生 reconcile、Hub Agent 摘要及 attention 使用共享读取。
4. Owner overlay 仅复制现有消费者需要的七个控制字段，包含完整 pending permissions。
   overlay 复制离开全局 observation map 锁；未变化的发布只做一次完整比较。
   发布与 pending/admitting 继续使用原串行化边界；完整完成快照仍提供给 wait 观察者。
   保留主线的原生子 Agent 只读登记、ready 过滤和配置绑定的 control 恢复逻辑。
   设计见 [ADR-210](../../decisions/providers/adr-210-committed-runtime-read-snapshots.md)。

## 验证

本地未运行完整 Rust workspace 测试。切换基线前及系统 Rust 1.99.1 的检查不作为最终版本验收结果。
以下 Cargo 命令均加 `PATH=/tmp/ait-rust-toolchain-1.98.1/bin:$PATH`，使用本地安装的官方
1.98.1 工具链；未修改系统工具链。

- `cargo test --locked -p persistence --lib -- --show-output`：83 通过、0 失败。
- `cargo test --locked -p domain --lib agent_runtime`：3 通过、0 失败、22 项未选中。
- `cargo test --locked -p daemon --bin daemon host::hub`：24 通过、0 失败、107 项未选中；
  包含轻量目录、attention、断线恢复、event identity、scope 和 late ACK。
- Provider：下列命令 127 通过、0 失败、1361 项未选中；包含权限、取消、完成、只读子 Agent、
  wait、自动归档、目录同步、历史恢复和分页的直接相关回归。
- `node node_modules/vitest/vitest.mjs run --pool=threads --testTimeout=180000 --teardownTimeout=120000 packages/protocol/src/messages.tool-call-schema.test.ts`：15 通过、0 失败。
- `cargo clippy --locked -p domain -p persistence -p provider -p daemon --all-targets -- -D warnings`：通过，无警告或错误。
- `cargo fmt --all --check`：通过；变更的 TypeScript 文件通过 oxfmt 和 oxlint。
- `node --test scripts/check-docs.test.mjs`：1 项测试通过；`node scripts/check-docs.mjs`：
  507 份 Markdown 本地链接验证通过。变更文档通过 oxfmt。

```sh
cargo test --locked -p provider --lib -- \
  local::tool_detail local::omp::stream local::omp::history \
  timeline_projection storage::timeline::projection \
  service::agent_runtime service::workspace_attention \
  service::agent_manager::ownership rpc::agent_runtime \
  service::agent_execution::tests::concurrency \
  service::agent_execution::tests::auto_archive \
  service::agent_manager::tests::auto_archive \
  service::agent_execution::tests::controls service::agent_execution::tests::waits
```

协议默认 fork worker 曾启动超时；threads 模式中，新旧案例均触及默认 5 秒时限，
没有 schema 比较失败。直接将当前源码与 AOT validator 编译为 Node ESM 后，4 个修复页和
4 个非法 content 形状全部通过正反断言（本次冷校验合计 6963.81 ms）。该窗口属于本次 Linux
开发验证，不与原报告的 Mac/模型等待拼接。协议用例以 threads 和显式冷校验时限重跑已通过，
不减少并发，也不以测试框架耗时推断生产 paint 或 API 延迟。

## PR CI 回归

提交 `648a2e3e1c485c185f331aec16e3e86c96e6e10a` 的 Linux Rust、UI、Web 和文档/格式任务通过；
[macOS headless 任务](https://github.com/ait-app/ait/actions/runs/41609740387/job/125828377111)
在 `host-link` 的 `persistent_access_refusal_probes_refresh_once_then_stops` 失败：
`task.is_finished()` 为 false，44 项测试中 43 项通过。

该测试使用 `advance()` 和固定次数的 `yield_now()` 后断言结束，但推进时间不保证所有已唤醒
定时器和任务完成。三个同类认证退出用例改为在原有 10/5 秒虚拟时间上限内等待 JoinHandle；
保留 `Unauthorized`、刷新次数和已关闭链路的断言。未修改业务状态机或降低测试并发。

- `cargo test --locked -p host-link --lib controller::tests::recovery -- --nocapture`：5 通过。
- `cargo test --locked -p host-link --lib`：44 通过。
- 同一构建产生的 `target/debug/deps/host_link-b77293d1836a0bac --quiet` 默认并发重复 20 轮，
  合计 880 项通过、0 失败；未设置 `--test-threads` 或减少测试并发。
- `cargo fmt --all --check` 和
  `cargo clippy --locked -p host-link --all-targets -- -D warnings`：通过。

## 性能测量

`registry::tests::performance::repeated_committed_snapshot_reads_avoid_all_payload_clones`
在相同 fixture 上比较拥有记录的 list 和共享 snapshot：257 条、每条 3072-byte payload、
200 次读取；另检查共享单记录读取。记录真实 Clone 次数，耗时不作为 CI 断言。
这是 registry 状态保持不变时的合成读负载，不能代表生产 API p95 或整条目录链路加速比例。

本次 fixture 实测：拥有记录的读取深克隆 **51,400 次**，共享视图及共享单记录读取 **0 次**。
200 次 list 总计 173,969 微秒，200 次 snapshot 总计 58 微秒。计时包含 clone 计数的原子操作，
运行于共享开发主机、dev profile 且其他测试并行；只作本地诊断，不据此宣称生产加速倍数。

## Test coverage

**not measured**：用户未要求覆盖率。本次仅执行受影响代码和直接相关行为的定向测试；
通过测试数量不是覆盖率百分比。若后续要求测量，应使用 cargo llvm-cov 限定实际受影响
crate/测试，记录可比基线、覆盖行数和可分享的 HTML artifact。

## 尚未验证或优化的边界

- 尚未在生产数据库或真实客户端部署验收；SQLite fixture 检查旧页、重启、重建、分页预算和
  后续完成/取消事件的连续 source coverage。
- 未测量生产 N/M/K 增长的 API 分位值。写入 staged map、完整 JSON 投影/比较、placement 扫描
  仍有成本；未改 Hub 的 outbox 事务/authority/ACK 锁边界，不将共享读取等同于完成全部 P2。
- 报告中 78.090 秒属于原生模型阶段的正文开始延迟。本次不能证明该等待缩短；
  admission/dispatch/native/commit/fetch/paint 跨端计时和 renderer/startup profile 仍需单独验收。
  没有调整用户模型、effort、网络或原生事件轮询节奏。
