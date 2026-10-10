# 周期任务按变更驱动：PR 验证

2026-10-10；源码提交 `6ed74f465ade7b06b615e237b0041280ed71761b`，基于 main `1fce5dbd2eccdb431a0726c9ce2bf65105d1cc60`。
测量树为 `4c0f20e3f05efe2f02d2085f283ecd78b67e7f9c`；随后只增加本报告、证据和索引，不改变 Rust 源码。

daemon 中按固定周期运行的任务改为由变更触发，或在每轮中只做必要工作。不改变依赖边界、
客户端协议和公开 RPC，因此不新增 ADR。

| 位置                     | 原行为                                                    | 现行为                                                                                       |
| ------------------------ | --------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Provider 调度器          | 每 25 ms 加锁并 fork 执行状态，检查自动归档               | 排入归档时由 `Notify` 唤醒；失败或预算不足的归档在每秒维护中重试                             |
| Provider 会话 lane       | 有原生会话即每 25 ms 完整处理一轮                         | 有活动 turn、待处理事件或前台待启动工作时 25 ms，其余 250 ms；请求启动 turn 时提前下一次处理 |
| 终端连接轮询（40 ms）    | 整份复制订阅状态和上次列表；每个列表订阅各读一次 registry | 只复制标识和游标；同轮列表共用一次 registry 读取，按 workspaceId 过滤时不读取                |
| 终端 reconcile（250 ms） | 无终端时也读 registry，并规范化所有 Workspace 路径        | 无终端时直接返回；只比较 workspaceId                                                         |
| 定时任务检查（1 s）      | 每秒复制全部 schedule 及其最多 4096 条运行记录            | 只读扫描；仅需标记完成时复制并提交                                                           |
| diff 订阅（200 ms）      | 每轮把完整 diff 序列化为 JSON 字符串比较                  | 直接比较结构化结果，变化时才编码；字段不变                                                   |
| 文件订阅（200 ms）       | 每轮复制路径字符串和失败令牌                              | 订阅开始时准备一次                                                                           |
| 旧版连接循环             | 每条消息额外清理一次过期上传                              | 保留 30 秒定时清理和上传帧处理时的清理                                                       |

空闲 Agent 的原生事件（例如模型信息和用量更新）最多延后 250 ms 处理；运行中的 turn 和排队输入
仍按 25 ms 处理。[ADR-081](../../decisions/workspace/adr-081-directory-change-push.md) 所述 25 ms
内部检查现只适用于有活动 turn 的会话。

## 验证

- `cargo test --workspace -- --test-threads=4`：2020 通过、0 失败、14 忽略；doctest 阶段通过。
- 覆盖率完整测试：2020 通过、0 失败、14 忽略，不含 doctest；同一会话测得的基线为 2014 通过。
- fmt、workspace build、严格 Clippy、文档本地链接和 diff 检查通过。
- 新增 6 个测试：归档唤醒及失败后可重试、活动 turn 判定、终端批量列表共用 registry 读取、
  无终端时 reconcile 跳过读取、定时任务检查在无需完成时不写入、diff 推送字段不变。

## Test coverage

Workspace 行覆盖率 **94.6788%（56278 / 59441）**，相对可比基线 **+0.0528 个百分点**。
provider **94.2258%（26795 / 28437）**，terminal **96.7193%（1592 / 1646）**，
schedule **97.6131%（777 / 796）**，filesystem **94.8567%（11582 / 12210）**，api **94.5455%（2132 / 2255）**。

命令：`cargo llvm-cov --workspace --html -- --test-threads=1`。
范围：全部 13 个 workspace package、默认 features、macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4，
无额外文件排除；14 项原生 Provider 用例按默认配置忽略。Doctest、shell/Python fixture
和上游二进制不在 Rust instrumentation 范围，Linux/Windows 未纳入本地测量。

基线为 main `1fce5dbd2eccdb431a0726c9ce2bf65105d1cc60`，在同一会话、同一平台和独立 worktree 中以相同测试范围测量，
workspace 为 94.6259%（56169 / 59359）。[共享证据](periodic-loops-pr-coverage-2026-10-10.json)包含精确命令、
源码指纹、逐 crate 计数、改动文件覆盖率和 LCOV 未覆盖行。本地 HTML 为 `target/llvm-cov/html/index.html`。

| Package     | 行覆盖率                  | 相对基线   |
| ----------- | ------------------------- | ---------- |
| api         | 94.5455%（2132 / 2255）   | -0.0024 pp |
| browser     | 97.5443%（715 / 733）     | +0.0000 pp |
| daemon      | 94.7520%（1318 / 1391）   | +0.0000 pp |
| domain      | 100.0000%（549 / 549）    | +0.0000 pp |
| filesystem  | 94.8567%（11582 / 12210） | -0.0189 pp |
| metadata    | 94.1562%（5462 / 5801）   | +0.0000 pp |
| model       | 96.2536%（1336 / 1388）   | +0.0000 pp |
| persistence | 95.8734%（1696 / 1769）   | +0.0000 pp |
| provider    | 94.2258%（26795 / 28437） | +0.1212 pp |
| relay       | 90.6561%（456 / 503）     | -1.1928 pp |
| schedule    | 97.6131%（777 / 796）     | +0.0151 pp |
| terminal    | 96.7193%（1592 / 1646）   | +0.2480 pp |
| voice       | 95.1605%（1868 / 1963）   | +0.0000 pp |

relay 未改动；其 `bridge.rs` 以及 deepseek_harness、git fetch 等未改动文件的计数随超时与并发时序波动。
filesystem 与 api 的小幅下降来自删除已覆盖的重复代码，分母同时减小。

改动行中仅 `crates/provider/src/service/agent_execution.rs` 第 377 行未覆盖：
归档预算不足或关闭失败后，在每秒维护中重试。`Owners` 的单元测试覆盖了重试资格判定；
后续修改归档调度时，补充可注入关闭失败的会话以端到端验证该路径。空闲 lane 的 250 ms
与活动 turn 的 25 ms 节奏由判定测试和现有 lane 行为覆盖，尚无基于暂停时钟的时序断言。
