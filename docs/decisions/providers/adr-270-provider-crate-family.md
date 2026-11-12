---
id: ADR-270
status: accepted
date: 2026-11-12
amends:
  - providers/adr-089-provider-owned-composition.md
  - daemon/adr-107-direct-imports-from-owning-crates.md
  - providers/adr-179-provider-owned-tests.md
  - providers/adr-249-omp-adapter-crate.md
paths:
  - crates/provider
  - crates/provider-omp
  - bins/daemon
  - crates/provider-core
  - crates/provider-native
  - crates/provider-builtin
  - crates/provider-minimax
---

# ADR-270：Provider 拆为契约、共享工具、逐 adapter、运行时与装配 crate

- 状态说明：决策已接受，按 [provider crate 拆分计划](../../plans/provider-crate-split.md)分步实施；
  依赖守卫已先登记目标 crate 及其依赖规则，各 crate 建立后其路径追加到本文和相关 ADR 的 `paths`。
- 修订：[ADR-089](adr-089-provider-owned-composition.md) 的装配归属（内置 adapter 列表移到
  `provider-builtin`，OMP 不再是宿主了解客户端类型的例外）；
  [ADR-107](../daemon/adr-107-direct-imports-from-owning-crates.md) 第 4 条（`provider` 可经只由
  `test-support` 启用的可选依赖使用 `persistence`）；
  [ADR-179](adr-179-provider-owned-tests.md)（adapter 的测试归该 adapter 的 crate）；
  [ADR-249](adr-249-omp-adapter-crate.md) 第 1 条的依赖、第 2 条的 daemon 注册与第 4 条的夹具例外。

## 背景

`provider` 是一个约 13 万行的编译单元，包含 ports 与 protocol 契约、八个原生 adapter、它们共用的
原生工具，以及运行时服务。类型检查本身很快：在 macOS arm64 上修改 `local/minimax.rs` 后
`cargo check -p provider --lib` 约 1.4 s；但构建测试二进制需要 34.1 s，修改
`ports/agent_session.rs` 需要 40.4 s，且 `npm run verify` 随后选中全部 1,696 个测试。
已拆出的 `provider-omp`（2.1 万行、245 个测试）修改后构建测试只需 4.9 s；但它仍链接整个
`provider`，修改 `provider` 内的 adapter 也会重建它的测试（5.6 s）。

2026-10-01 以来触及 provider 家族的 159 个非合并提交中，50 个（31%）只改一个 adapter
（可附带共享工具），28 个（18%）只改运行时，74 个（47%）跨越 adapter 与运行时或契约。
按 crate 拆分后，约三分之一的修改只需构建和运行一个 adapter 的测试；其余修改也构建更小的
测试二进制，Codex 的 `webrtc`、`rtc` 与 `opus2` 不再阻塞运行时和其他 adapter 的编译。

在 `f3a2c3d3` 扫描 `crates/provider/src` 的全部 `crate::` 与 `super::` 路径，生产代码的耦合很窄：

- adapter 到运行时只有 `service::bot_execution::{BotSessionSupport, InstructionDelivery}`，
  由 `ports/agent_session.rs`、`local/claude.rs` 与 `local/codex.rs` 引用；
- 运行时到 adapter 只有两处 `local::pi::PROVIDER`，以及 `composition.rs` 中的七个客户端类型；
- adapter 之间、运行时到共享工具之间没有生产依赖。

测试的耦合更宽：约 60 个 adapter 测试文件使用 `AgentManager`、Timeline、`AgentExecution`
等运行时内部项，若干运行时测试构造 Codex 以外的真实 adapter。

## 决策

目标依赖图（实线为普通依赖，`..>` 为仅 dev 依赖）：

```
domain <- model <- provider-core <- provider-native <- provider-<adapter> x8
                        ^                                     ^
                        |                                     |
                     provider (runtime)  <----------  provider-builtin
                        ^      ^                              ^
                       bot    api                           daemon

dev-only:  provider-<adapter> ..> provider        (adapter 测试驱动真实运行时)
           provider           ..> provider-codex  (运行时执行测试驱动 Codex 夹具)
           api                ..> provider-builtin
```

各 crate 的内容（路径相对于拆分前的 `crates/provider/src`，行数与测试数为 `f3a2c3d3` 的测量）：

| Crate                                                                                                                                            | 内容                                                                                                                                                                                                                                | 非测试行数                               | 测试数                         |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ------------------------------ |
| `provider-core`                                                                                                                                  | 下方契约表中移出的部分                                                                                                                                                                                                              | 约 1.6k                                  | 约 20                          |
| `provider-native`                                                                                                                                | `local/{acp_config,acp_permissions,acp_streaming,acp_transport,compaction,configuration,diagnostics,elicitation,images,metadata_process,notes,process_group,prompt_blocks,sandbox,session_preview,summary_model,tool_detail,usage}` | 4.9k                                     | 131                            |
| `provider-codex`、`provider-pi`、`provider-opencode`、`provider-deepseek-harness`、`provider-claude`、`provider-antigravity`、`provider-minimax` | `local/<adapter>.rs` 与 `local/<adapter>/`                                                                                                                                                                                          | 8.0k、4.7k、5.6k、4.5k、5.3k、2.0k、1.0k | 272、306、156、113、85、74、23 |
| `provider-omp`                                                                                                                                   | 已拆出，只调整依赖                                                                                                                                                                                                                  | 9.8k                                     | 267                            |
| `provider`（运行时，名称不变）                                                                                                                   | `service`、`rpc`、`storage`、`dispatch`、`connection`、`capabilities`、`timeline_projection`、`skills`、`support`、`diagnostics`、`installation`、`composition`、`testing`，以及 `ports` 与 `protocol` 中只属于运行时的部分         | 约 24k                                   | 约 590                         |
| `provider-builtin`                                                                                                                               | 新增：内置 adapter 列表及其可执行文件默认值与覆盖变量，以及需要多个真实 adapter 与真实运行时协作的测试                                                                                                                              | 约 0.1k                                  | 约 6                           |

契约划分：一个模块的每一项都被 adapter 使用或出现在 port 签名中时整体移入 `provider-core`；
同时包含 adapter 使用的值和 RPC 请求类型的模块按项拆分，两半在各自 crate 中保留原模块名。

| 模块                                                                                        | 移入 `provider-core`                         | 留在 `provider`                                                                                                            |
| ------------------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `ports/{agent_session,account_usage,controls,diagnostics,environment,native_history}`       | 整体                                         | —                                                                                                                          |
| `ports/bot_session`                                                                         | 整体                                         | —                                                                                                                          |
| `ports/usage_ledger`                                                                        | `UsageBoundary`、`RequestUsage` 及其测试     | `UsageLedger`、`NoopUsageLedger`、`CallSource`、`CallBegin`、`CallFinish`、查询与行类型                                    |
| `ports/{agent,mcp_scope}`                                                                   | —                                            | 整体                                                                                                                       |
| `protocol/{prompt,usage,agent_config}`                                                      | 整体（`ConfigPatch` 携带 `NullableSetting`） | —                                                                                                                          |
| `protocol/provider`                                                                         | `Details`                                    | `ListRequest`、`FeaturesRequest`、`SnapshotRequest`、`RefreshRequest`                                                      |
| `protocol/timeline`                                                                         | `NativeItem`                                 | `Cursor`、`Direction`、`Projection`、`FetchRequest`、`SearchRequest`、`AppendRequest`、`PluginItem`、`SubscriptionRequest` |
| `protocol/{agent,agent_execution,agent_lifecycle,controls,creation,native_sessions,resume}` | —                                            | 整体                                                                                                                       |

1. **运行时保留 `provider` 这个名称。** 改名要修改 `bot`、`api`、`daemon` 中的 43 个文件和
   84 个 ADR 的 `paths`，没有收益。
2. **adapter 与运行时之间没有任何方向的生产依赖。** adapter 的生产依赖只有 `provider-core`、
   `provider-native` 与 `domain`；只有 `provider-codex` 因 `model::realtime` 额外依赖 `model`。
   这也是第 5 条的前提：链接运行时的 adapter 不能用于运行时自身的测试，因为 `provider` 的
   测试构建与该 adapter 链接的副本会定义两个不同的 `AgentClient` trait。
3. **`Providers` 作为容器留在运行时，`provider-builtin` 拥有内置列表。**
   [ADR-249](adr-249-omp-adapter-crate.md) 加入的 `Providers::with` 已能接收其他 crate 的 adapter。
   过渡期间 `Providers::configured` 只构造仍在 `provider` 中的 adapter，`provider_builtin::configured`
   追加已拆出的 adapter；最后一个 adapter 移出后，`configured` 由 `Providers::empty` 取代。
   daemon 只调用 `provider_builtin::configured`，不再单独注册 OMP。`summary_generation::Generation`
   与 `UsageLedger` 保持 crate 私有，adapter 模块也不临时公开。注册顺序不可观察：摘要生成器和
   catalog 都按名称索引 adapter。
4. **adapter 测试留在 adapter 的 crate，只经 `provider::testing` 访问运行时**（feature
   `test-support`，见 ADR-249 第 4 条）。缺少的操作在该模块中增加委托包装，不为测试把运行时项改成
   `pub`。`provider::testing` 可以通过只由 `test-support` 启用的可选依赖使用 `persistence`，
   现有基于持久化的测试 helper 因此原样移入。
5. **`provider` 只 dev 依赖一个 adapter：`provider-codex`。** 131 个
   `service/agent_execution/tests` 用例和 manager 测试驱动 Codex 的 Python 夹具。`verify.mjs`
   沿 dev 边选择测试，运行时的 dev 依赖每多一个 adapter，修改该 adapter 时就会选中整个家族。
6. **夹具归拥有该假程序或产出该数据的 crate。** 原生假程序和 adapter 专用 JSON 移到
   `crates/provider-<adapter>/tests/fixtures/`；`legacy_tool_details.json` 与三个 `paseo-*.json`
   留在 `crates/provider/tests/fixtures/`。这取代 ADR-249 第 4 条中两个 OMP JSON 留在
   `provider` 的例外。
7. **构造非 Codex adapter 的运行时测试一分为二。** 运行时一半使用桩 `AgentClient`（如这些测试
   模块中已有的 `Client` 与 `ImportClient`）；adapter 一半在 adapter 自己的测试中断言该 adapter
   的 port 值。需要多个真实 adapter 与真实运行时同时参与的用例移到 `provider-builtin`。
8. **以第一个按统一步骤拆出的 adapter 作为检查点。** 它拆出后先对照上文测量，再继续拆分其余
   adapter。

依赖守卫（`bins/daemon/tests/dependencies.rs`）按下表登记这些 crate；“仅 dev”一列中的依赖只能以
dev 依赖声明，其余 crate 的 `persistence` 依赖规则不变。包内 `test-support` 以外的任何 feature
列出 `test-support` 都视为违规，因为它会让生产构建启用测试入口。判断可选 `persistence` 依赖由哪些
feature 启用时，`dep:persistence` 和强形式 `persistence/<feature>` 都计入，弱形式
`persistence?/<feature>` 不启用依赖，不计入：

| Package                     | 允许的 workspace 依赖                                                                  | 仅 dev                                                                                                         |
| --------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `provider-core`             | `domain`、`model`                                                                      | —                                                                                                              |
| `provider-native`           | `domain`、`provider-core`                                                              | —                                                                                                              |
| 七个新 `provider-<adapter>` | `domain`、`model`、`provider-core`、`provider-native`、`provider`、`persistence`       | `provider`、`persistence`；除 `provider-codex` 外还有 `model`                                                  |
| `provider`                  | `domain`、`model`、`persistence`、`provider-core`、`provider-native`、`provider-codex` | `provider-codex`；`persistence`，除非该依赖可选、`test-support` 列出 `dep:persistence` 且没有其他 feature 列出 |
| `provider-builtin`          | `domain`、`model`、`persistence`、`provider`、`provider-core`、八个 adapter            | `persistence`                                                                                                  |
| `bot`                       | 原有列表加 `provider-core`                                                             | —                                                                                                              |
| `api`                       | 原有列表加 `provider-core`、`provider-builtin`                                         | `provider-builtin`                                                                                             |
| `daemon`                    | 原有列表加 `provider-core`、`provider-builtin`                                         | —                                                                                                              |

`provider-omp` 在拆分过程中先增加 `provider-core`，再改用上表 adapter 一行的规则；daemon 不再
注册 OMP 后，`daemon → provider-omp` 从守卫中移除。

## 后果与限制

修改单个 adapter 只构建并运行该 adapter 的测试；修改契约或共享工具会选中整个家族，这与现状相同。
新 adapter 加入 `provider-builtin` 即可，宿主不了解任何客户端类型。行为、wire 格式、持久化格式、
环境变量和 provider id 都不变。

不在本决策范围内：继续拆分运行时（`skills`、`storage`、`timeline_projection` 保留在 `provider`）；
移除运行时代码中按字符串识别具体 provider 的七处特例（它们不是编译期依赖）；用脚本化客户端替换
运行时测试中的 Codex 夹具。

`sandbox/linux` 和若干 adapter 含 `cfg(target_os = "linux")` 代码，移动这些代码的步骤只在 macOS
本地验证不够，需要 CI 的 Linux Rust 任务通过。
