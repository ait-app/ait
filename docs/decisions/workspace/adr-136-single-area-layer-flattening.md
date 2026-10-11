# ADR-136：拍平单功能区层并收口重复助手

- 状态：接受
- 日期：2026-10-11
- 修订：[ADR-131](adr-131-metadata-capability-groups.md) 与
  [ADR-113](adr-113-filesystem-capability-groups.md) 中 `crate::<group>::<layer>::<area>` 的
  单功能区形态：一层只承载一个功能区时去掉多余的 `<area>` 段，路径为
  `crate::<group>::<layer>`。能力组集合、组间 `ports`/`protocol` 约定、对外服务与方法
  集合不变。

## 背景

ADR-131 与 ADR-113 把 metadata 与 filesystem 改为“能力组 → 层 → 功能区”。迁移后留下
一批单功能区层：`<layer>.rs` 只有文档注释和一个 `mod <area>;`，`<layer>/<area>.rs` 才是
实现，例如 `metadata::worktrees::ports::worktrees`、`filesystem::forge::ports::forge`、
`provider::skills::service::skills`。这些中间层不承载任何行为，只增加一级路径；
`metadata/src/worktrees/ports.rs` 的 TODO 已记录该问题。

审计还发现同类问题不在少数：34 个纯单功能区层（metadata 19、filesystem 11、provider 4），
以及三处被逐字复制的 RPC/ACP 助手。

## 决策

1. 拍平 34 个纯单功能区层：`src/<a>/<layer>.rs` 与 `src/<a>/<layer>/<area>.rs` 合并为
   `src/<a>/<layer>.rs`，`<area>` 的子模块（如 `tests/`）上移到 `<layer>/` 下。判定标准
   严格限定为“包装文件只有文档注释与一个 `mod` 声明”，因此 `support`、`dispatch` 这类
   真正的多子模块层不受影响。
2. 路径缩短一段，调用处按 [ADR-107](../daemon/adr-107-direct-imports-from-owning-crates.md)
   直接引用，不保留旧路径转发。例如：

   | 原路径                                        | 新路径                          |
   | --------------------------------------------- | ------------------------------- |
   | `metadata::worktrees::ports::worktrees`       | `metadata::worktrees::ports`    |
   | `metadata::labels::service::workspace_labels` | `metadata::labels::service`     |
   | `metadata::daemon::connection::daemon`        | `metadata::daemon::connection`  |
   | `filesystem::forge::ports::forge`             | `filesystem::forge::ports`      |
   | `filesystem::git::rpc::checkout`              | `filesystem::git::rpc`          |
   | `filesystem::files::connection::files`        | `filesystem::files::connection` |
   | `provider::skills::service::skills`           | `provider::skills::service`     |

3. `filesystem::files::connection` 拍平后会包含同名子模块，内层 `connection` 改名 `state`
   （每物理连接的订阅与上传状态）。对外仍经
   `files::connection::{FileConnection, FileRequest}` 暴露，调用方无感。
4. 同批收口两类重复助手，行为不变：
   - 新增 `model::rpc`（`decode`/`encode`，映射 `model::ErrorCode`），供 provider 与
     filesystem 中原先各自定义的三处使用；
   - 新增 metadata、filesystem、provider 的 `support::rpc`（`decode`/`encode`），以及
     metadata、provider 的 `support::time::now_timestamp`，分别承载各 crate 自己的
     `ErrorCode` 与毫秒级 RFC 3339 时间格式。

   各 crate 继续按 [ADR-095](../daemon/adr-095-component-method-declarations.md) 各自提供
   `implemented_methods`/`installed_methods`。曾考虑把七个 `installed_methods` 里那句
   `.filter(move |_| installed)` 也收进 `model`，但过滤器只有一行，收口不减少代码、只增加
   一层间接并让调用点不再自解释，因此保持各 crate 原样。

5. provider 新增 `local::acp_config` 与 `local::acp_permissions`：两个 stdio ACP 适配器
   （`opencode`、`deepseek_harness`）逐字相同的 `option`/`choices`/`state`/`text` 与
   `capture`/工具审批 `resolve` 收口到共享模块。适配器的 `config` 模块保留
   `pub(super) use` 门面，与既有的 `deepseek_harness::transport` 一致；elicitation 表单
   支持仍只由 `opencode` 拥有。

## 后果与验证

单功能区不再有中间层，层本身就是功能区；共享助手各自只有一个实现。模块边界测试
`crates/metadata/tests/module_boundaries.rs` 与 `crates/filesystem/tests/module_boundaries.rs`
的“组 → 层”判定规则无需改动，测试继续通过。

验证：`cargo fmt --all --check` 通过；`cargo clippy --locked --workspace --all-targets
--all-features`（`all` + `pedantic`）无告警；`cargo build --locked --workspace` 通过。
测试合计 1982 项全部通过：model、metadata、filesystem、provider、api、schedule、terminal、
browser、voice 的单元测试 1835 项，daemon 单元测试 32 项，metadata 与 filesystem 的模块
边界测试各 2 项，daemon 依赖方向测试 15 项，daemon 端到端 `process` 集成套件 96 项。
