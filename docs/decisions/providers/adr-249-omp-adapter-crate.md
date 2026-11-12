---
id: ADR-249
status: accepted
date: 2026-11-11
amends:
  - providers/adr-089-provider-owned-composition.md
  - providers/adr-179-provider-owned-tests.md
  - daemon/adr-116-crate-visibility.md
paths:
  - crates/provider
  - crates/provider-omp
  - bins/daemon
  - crates/provider-native
  - crates/provider-core
---

# ADR-249：OMP 适配器独立为 `provider-omp` crate

- 修订：[ADR-089](adr-089-provider-owned-composition.md) 的装配归属、
  [ADR-179](adr-179-provider-owned-tests.md) 的测试位置，以及
  [ADR-116](../daemon/adr-116-crate-visibility.md) 第 3 条的测试入口可见性。
- 后续修订：[ADR-248](adr-248-omp-paseo-native-rpc.md) 删除了 ACP 桥接和原定的 `omp-acp`
  二进制，并收窄了公开项；本文已按其结果更新。

## 背景

OMP 适配器原先位于 `provider` 内部，与运行时和其他 adapter 同属一个编译单元；`provider`
运行时直接持有这个具体 adapter。拆分的最初动机是为外部 ACP 客户端提供不依赖 daemon 的
`omp-acp` 二进制。ADR-248 删除 ACP 桥接后，这个动机已不存在，但独立 crate 仍然保留：
适配器的生产代码只用到 `provider` 的 ports、protocol 和共享原生工具，不依赖运行时服务；
由 Cargo 强制这条边界，也是 [provider crate 拆分计划](../../plans/provider-crate-split.md)
的先行实现。

## 决策

1. OMP 适配器移入库 crate `provider-omp`，生产依赖为 `provider` 与 `domain`。它只公开
   `OmpClient`（`new`、`with_image_directory`）和 `installed_program(environment)`：
   设置了 `AIT_SERVER_OMP_BIN` 时返回该值，否则返回 PATH 中的 `omp`。`provider` 不依赖
   `provider-omp`。OMP 的会话实现和沙箱状态目录都在 `provider-omp` 中；`provider` 只在
   provider catalog 中按名称保留 `omp` 的展示标签、描述与默认模式。
2. `provider::Providers::with` 接收外部 crate 构造的 adapter，并提供共享图片目录。
   daemon 在 `compose_providers` 中追加 OMP；其余内置 adapter 仍由 `Providers` 装配。
   OMP 是 ADR-089「宿主不了解客户端类型」的唯一例外。
3. `provider` 只公开 OMP 生产代码用到的项：
   - 模块 `ports::{account_usage, agent_session, controls, diagnostics, environment, native_history, usage_ledger}`；
   - 模块 `protocol::{agent_config, prompt, provider, timeline, usage}`；
   - 模块 `local::{configuration, diagnostics, images, process_group, sandbox, session_preview, usage}`；
     其中 `usage::requests::{pi_request, response_id}` 与 `usage_ledger::RequestUsage` 供 OMP 生成用量账本分段。

   这些模块中，只有 `provider-omp` 使用的项，以及公开签名引用的类型（如 `UsageIdentity`、
   `PreparedUsage`、`ConfigPatch`、`HistoryError`）是 `pub`。`sandbox::StateLocation::tree`
   公开，供 `provider-omp` 声明自己的状态目录；`ImageStore` 的 `new`、`render` 与
   `UNAVAILABLE_IMAGE` 公开，供它渲染原生图片，`ImageStore::source` 仍只在 `local` 内可见。
   运行时服务、存储以及 `local` 下其他模块的可见性不变。

4. OMP 测试和离线夹具随适配器移入 `provider-omp`。与 TypeScript 客户端共享的
   `omp_dialogs.json`、`omp_question.json` 留在 `crates/provider/tests/fixtures/`。
   需要 `AgentManager` 或 Timeline 的测试使用 `provider::testing`。该模块只在 `test-support`
   feature（或 provider 自身的测试）下编译，以包装类型和函数委托给 `pub(crate)` 运行时操作，
   不把这些操作变成生产 API。这修订 ADR-116 第 3 条：`cfg(test)` 测试入口可以经该 feature
   供 adapter crate 的测试使用。
5. 依赖守卫登记 `provider-omp → provider/domain`（`model`、`persistence` 仅测试使用）和
   `daemon → provider-omp`，并断言 `provider` 不依赖 `provider-omp`、`provider-omp`
   不依赖 daemon。非 dev 依赖边启用 `test-support`，或任何 package feature 转发
   `*/test-support`，都视为违规。

## 后果与限制

daemon 的 provider 列表和状态仍包含 `omp`（状态按名称排序）；`Providers` 内部顺序中 OMP 移到最后。
发布产物不变，OMP 只随 daemon 提供。

本决策没有先拆出 `provider-core` 与 `provider-native`，共享工具暂时仍由 `provider` 公开。

验证覆盖依赖守卫、`provider` 的装配与 `testing` 测试、daemon 的配置与进程测试、
workspace clippy `-D warnings`，以及不启用 dev feature 的 `provider` 生产构建。
