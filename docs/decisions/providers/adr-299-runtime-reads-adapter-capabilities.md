---
id: ADR-299
status: accepted
date: 2026-11-23
amends:
  - providers/adr-270-provider-crate-family.md
  - providers/adr-177-codex-restart-continuation.md
  - daemon/adr-109-local-incident-evidence.md
paths:
  - crates/provider/core
  - crates/provider/runtime
  - crates/provider/builtin
  - crates/provider/codex
  - crates/provider/claude
  - crates/provider/opencode
  - crates/provider/deepseek-harness
  - crates/provider/antigravity
  - crates/provider/omp
  - crates/provider/minimax
  - crates/provider/pi
  - bins/daemon
---

# ADR-299：运行时按 adapter 声明的能力决策，不比较 provider id

- 修订：[ADR-270](adr-270-provider-crate-family.md)“后果与限制”中留给后续的运行时字符串特例已移除；
  [ADR-177](adr-177-codex-restart-continuation.md) 第 1 条的“Codex running 记录”改为
  “声明继续中断轮次的 adapter 的 running 记录”，目前仍只有 Codex；
  [ADR-109](../daemon/adr-109-local-incident-evidence.md) 的 Harness 来源改由 adapter 声明。

## 背景

ADR-270 之后，adapter 只在 `provider-builtin` 注册，但运行时 `provider` 的生产代码仍有 9 处按
provider id 字符串分支：`diagnostics.rs` 的六项诊断来源表（覆盖变量、默认程序、日志目录）与
OpenCode 的日志目录查询；`provider_catalog.rs` 为 DeepSeek Harness、Antigravity、OMP 与 MiniMax Code
补充标签、说明和默认模式；重启恢复只对 Codex 排入继续指令；OpenCode 有活动子会话时跳过原生子会话发现；
导入会话缺少模式时 Claude 记 `default`、其余记 `read-only`。新增或修改 adapter 时必须记得同步修改运行时，
编译器和测试都不会提醒。端口上已有按行为命名的声明（`supports_*`、`cwd_lookup`、`preset_backend`），
这些特例是同一模式的遗漏。

## 决策

1. **`AgentClient` 用按行为命名的方法声明这些决定，默认值保持非特殊 adapter 的原行为。**
   `continues_interrupted_turns`（默认 `false`）、`live_subagents_are_complete`（默认 `false`）、
   `imported_session_mode`（默认 `"read-only"`）、`catalog_presentation`（默认 `None`，返回
   `provider_core::ports::catalog::CatalogPresentation`：标签、说明与 `DefaultMode`）和
   `harness_evidence`（默认 `None`，返回 `provider_core::ports::harness_evidence::HarnessEvidenceSource`：
   程序覆盖变量、默认程序、日志目录覆盖变量与 `NativeLogs`）。方法名描述行为而不是 provider，
   声明由拥有该行为的 adapter crate 实现。
2. **运行时只读取声明。** Catalog 条目、重启继续、子会话发现、导入模式与 Harness 证据都经端口方法决定；
   运行时生产代码不再出现内置 provider id。唯一保留的是 `storage/agents.rs` 旧 Agent 目录 schema 中的
   `CHECK(driver='codex')`：它约束已持久化的行，修改需要存储迁移，不是运行时分支。
3. **Harness 证据由已注册的 adapter 组成。** `Providers::harness_evidence` 按注册顺序收集声明来源，
   daemon 启动诊断收集器时从 `provider_builtin::providers` 取得它。`provider-builtin` 的注册顺序因此
   决定报告中 Harness 段的顺序，调整为 OpenCode、Codex、Claude、DeepSeek Harness、Antigravity、
   MiniMax Code、OMP、Pi，使报告与此前逐字节一致；注册顺序对其他行为仍不可见（Agent 管理器与摘要
   生成器按 provider id 建索引）。
4. **守卫测试防止回退。** `provider-builtin` 的 `runtime_provider_ids` 测试从已注册 adapter 取得
   provider id，扫描 `crates/provider/runtime/src` 中测试模块以外的源码，发现以单引号或双引号引用的内置
   id 即失败；允许清单只有第 2 条的 schema 约束，清单项不再出现时同样失败。新 adapter 注册后自动纳入检查。

## 后果

新增 adapter 只需在自身 crate 实现端口声明并在 `provider-builtin` 注册，运行时不需修改。Wire、RPC
负载、持久化格式与诊断输出不变：`provider-builtin` 的 `runtime_decisions` 测试逐项断言每个内置 adapter
的声明等于迁移前运行时写死的值。端口新增五个有默认实现的方法，现有及第三方 adapter 不实现也能编译，
但若其行为依赖原先按 id 的特例，必须显式声明。

DeepSeek Harness 与 Antigravity 的诊断仍探测 `AIT_SERVER_*_BIN` 或 PATH 中的 `dsh`、`agy`，与它们实际
启动时查找已安装程序的逻辑不同；这是迁移前已有的差异，本决策保持不变。

不在本决策范围内：TypeScript 客户端（`apps/`、`packages/`）、`crates/api` 与 `crates/bot` 中的 provider id
判断，以及 adapter 内部使用自身 id 的代码。
