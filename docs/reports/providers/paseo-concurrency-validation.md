# Paseo 并发改造验证

日期：2026-10-07。平台：macOS arm64。
初期实现基线：`5df233ac2f87cecda5f1a1ee99c003a15522dc49`；PR 已整合 `aa6d8d820c139f3f761d89c09a7608ecdc5629c9` 的最新 main。
范围：[并发修改清单](../../plans/paseo-agent-concurrency.md) C01–C12 的实现、定向验证和 PR 提交检查；架构依据 [ADR-091](../../decisions/providers/adr-091-independent-session-execution.md)。

## 交付行为

会话生命周期、前台操作与事件归各自的异步所有者，阻塞存储不占 Tokio reactor。目录、已加载 timeline、批量订阅身份解析与 watch wait 使用独立路径。AIT ID 和 native ID 共用 writer 归属，归档/删除/Workspace 退役及自动清理通过相关会话屏障协调。

Catalog 普通快照立即返回缓存或 loading，发现按 Provider/scope 合并、每 Provider 并发 4，并在后台逐项更新。客户端等待目标刷新终态，使用 generation/revision 拒绝旧 GET 或 push 回滚。命令、live session、history、查询和缓存上限保留为共享预算。

新增 debug tracing 包含请求排队与执行时间、native create/resume/restore/history 耗时、Catalog 预算等待与并发数量、会话预算数量和事件提交耗时，不包含输入内容或凭据。

## PR 提交验证

`SHERPA_ONNX_ARCHIVE_DIR=/private/tmp cargo test --locked --offline --workspace -- --test-threads=1`：**1,888 passed、0 failed、7 ignored**，包含 93 个实际 daemon 进程与 WebSocket 测试。
`cargo build --locked --offline --workspace`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 均通过。构建使用已下载的官方 Sherpa ONNX 1.13.8 arm64 静态库，验证了完整原生链接；不再依赖 `DOCS_RS=1` 跳过链接。

当前源码的前端/协议定向测试为 70 通过，protocol/client 构建和 mobile 全量 typecheck 通过。最新 main 新增的 `expo-web-browser` 按 lockfile 的版本与 SHA-512 校验补齐，只写入忽略的依赖目录。

集成保留 ADR-088 的物理连接异步响应及 64 个响应槽预算，替换其串行 Catalog。目录独立读取使用同一份已提交 runtime overlay，overlay 发布后唤醒目录订阅，避免 durable status 与尚未发布的 activeTurn 混合；缺失 Agent 的 get 保留原有内联空结果。旧 discovery 集成测试改为显式等待查询或检查快照终态，refresh 测试按响应与事件类型收尾，允许后台逐项推送。

初次全量运行发现上述投影、缺失身份和同步快照假设，均已修正。一次四线程目录同步夹具超时，单独复核通过；最终完整运行使用一线程，未放宽 timeout 或增加 ignored。7 个原有忽略测试要求本机 CLI、真实认证或显式安装配置，详见覆盖率证据中的清单。

## 初期定向测试（5df233ac 基线）

| 命令 | 范围 | 结果 |
| --- | --- | --- |
| `cargo test -p provider service::agent_execution -- --test-threads=4` | 执行器、输入、控制、订阅、wait 与并发竞态 | 76 通过 |
| `cargo test -p provider service::provider_catalog -- --test-threads=4` | 快照、缓存、发现、失败隔离及并发预算 | 23 通过 |
| `cargo test -p provider service::agent_manager -- --test-threads=4` | writer 所有权、别名/屏障、持久化失败与清理 | 59 通过 |
| `cargo test -p provider storage::timeline -- --test-threads=4` | epoch、历史替换、进度和输入存储 | 27 通过 |
| `cargo test -p provider connection::directory -- --test-threads=4` | 目录序列与订阅 checkpoint | 3 通过 |
| `npm run test --workspace @ait/mobile -- --project unit src/data/providers-snapshot.test.ts src/hooks/use-providers-snapshot.test.ts src/runtime/rust-daemon/messages.test.ts` | 快照、刷新、版本与 Rust transport 转换 | 60 通过 |
| `npm run test --workspace @ait/protocol -- src/messages.providers-snapshot.test.ts` | schema、兼容字段及生成的 validator | 10 通过 |

测试使用内存/临时存储、可控制的 Provider gate 和原生协议 fixture，未连接真实模型服务或使用账户凭据。新并发测试通过通知和 semaphore 控制阻塞点，timeout 只作测试保险或验证 wait deadline；没有用固定 sleep 推断提速。

新增行为证据位于 [执行器并发测试](../../../crates/provider/src/service/agent_execution/tests/concurrency.rs)、[Catalog 并发测试](../../../crates/provider/src/service/provider_catalog/tests/concurrency.rs)、[身份与屏障测试](../../../crates/provider/src/service/agent_manager/ownership/tests.rs)和[客户端快照测试](../../../apps/mobile/src/data/providers-snapshot.test.ts)。覆盖的关键交错包括：

- 慢 create 时另一条会话能读取、发送、完成；慢 discovery 时目录读取和 native 事件继续。
- 重复 resume 只有一个 writer；恢复中的 wait 保持未完成，重复冷历史只读取一次原生历史。
- archive/delete 等待相关恢复，其他会话继续；Workspace 退役包含 factory 尚未完成的创建。
- 自动归档等待创建中的子会话并关闭相关 writer；删除后迟到的历史不重建身份。
- 已排队的只读请求在 history 加载标记改变后仍只读取 durable 投影；输入派发只读本身份的首条 FIFO，其他身份损坏的 prompt 不影响调度。
- factory 返回后发现 Workspace 已退役，创建失败且 writer 关闭；晚发现的退役 scope 持续拒绝 launch，直到屏障解除。
- 全局 32 个会话预算包含在途 factory，删除释放容量；别名冲突拒绝第二个所有者。
- 同 scope 发现去重、刷新合并为一次后续发现、每 Provider 最多并发 4 个 scope；另一个 Provider 独立完成。
- refresh ack 后等待 ready/error 终态；旧 revision 的 GET/push 不覆盖新快照，daemon generation 改变后接受新快照。

原有测试继续验证 terminal 落盘失败不公布成功、失败关闭保留 writer、配置与权限事件顺序、rewind 替换与 epoch、未知输入接纳不重发、voice turn 取消及停机后清理。

## 构建与静态检查

- `cargo fmt --all --check`、`cargo clippy -p provider --all-targets -- -D warnings`：通过。
- `npm run build --workspace @ait/protocol`、`npm run build --workspace @ait/client`、`npm run typecheck --workspace @ait/mobile`：通过。
- `node_modules/.bin/oxfmt --check apps/mobile/src/data/providers-snapshot.ts apps/mobile/src/data/providers-snapshot.test.ts packages/protocol/src/messages.ts packages/protocol/src/messages.providers-snapshot.test.ts`：通过。
- `node_modules/.bin/oxlint apps/mobile/src/data/providers-snapshot.ts apps/mobile/src/data/providers-snapshot.test.ts packages/protocol/src/messages.ts packages/protocol/src/messages.providers-snapshot.test.ts`：通过。
- `npm run check:docs`、`git diff --check`：通过。
- 初期受限沙箱无法下载 Sherpa，曾用 `DOCS_RS=1 cargo check --workspace` 做类型检查。PR 准备时已下载官方依赖并通过普通完整构建和原生链接。

前端依赖复用本机已有 node_modules，并将 `@ait/protocol`、`@ait/client` 指向当前工作树。生成 validator 和 dist 为忽略的构建产物，未添加运行数据库或凭据。

## Test coverage

| 范围 | 已覆盖 / 总行数 | 行覆盖率 |
| --- | --- | --- |
| Rust workspace（13 crate） | 53,543 / 56,775 | 94.3074% |
| provider | 25,581 / 27,305 | 93.6861% |

普通与 coverage 插桩全量运行均为 **1,888 passed、0 failed、7 ignored**，测试数量与行覆盖率分别统计。HTML 已生成并审阅于 `target/llvm-cov/html/index.html`；[可审查 JSON 证据](paseo-concurrency-coverage-2026-10-07.json)随 PR 提交，包含逐 crate、逐文件、修改模块统计、原始报告 hash、精确命令、源码指纹及忽略清单。源码指纹为 `9e9fc42dc56bf8fa3f14bc943a6e7974b34969136d7567e2f0fe82b32f2c7683`。

测量环境为 macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、默认 features 与默认文件过滤，无额外排除或新忽略测试；测试并发为 1。普通测试包含 doctest，coverage 不插桩 doctest 或原生 C/C++ 依赖。没有对整合后的目标 main 使用相同配置重新测量，因此不计算可比基线差值。

```sh
SHERPA_ONNX_LIB_DIR="$PWD/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib" cargo llvm-cov --locked --offline --workspace --html -- --test-threads=1
SHERPA_ONNX_LIB_DIR="$PWD/target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib" cargo llvm-cov report --locked --offline --json --output-path /private/tmp/ait-pr-coverage-raw.json
```

尚未覆盖 Catalog 30 秒超时降级、30 秒空闲任务回收、初始 startup barrier 等待、debug tracing 分类及部分 poisoned-lock/JoinError/饱和拒绝分支。可通过可控时钟、启动屏障、故障注入及 debug subscriber 补充。未验证 Windows/Linux、真实模型服务或真实远程链路；本机 daemon process E2E 和 Sherpa 原生链接已验证。

## 性能验收限制

受控测试证明慢操作不再造成跨会话整段排队，未测量真实远程 Workspace 打开的 p50/p95，因此没有给出提速倍数。网络建连、目录同步、长历史每页投影与浏览器渲染仍需按[打开性能方案](../../plans/remote-workspace-open-performance.md)分别采样。

同一会话的 writer 修改仍保序；长 native 操作会让该会话的其他写操作等待。共享存储短事务受自身锁限制。显式模型/模式/available 查询（包括 daemon snapshot 的完整可用性查询）仍可等待目标 discovery，但其等待已经离开会话执行路径；普通 Provider 快照读取立即返回。
