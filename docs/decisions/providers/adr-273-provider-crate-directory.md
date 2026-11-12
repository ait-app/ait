---
id: ADR-273
status: accepted
date: 2026-11-13
amends:
  - daemon/adr-072-workspace-names-and-documentation.md
  - providers/adr-270-provider-crate-family.md
paths:
  - Cargo.toml
  - crates/provider
---

# ADR-273：Provider crate 家族收拢到 `crates/provider/`

- 修订：[ADR-072](../daemon/adr-072-workspace-names-and-documentation.md) 第 1 条的“目录与包名一致”
  对 provider 家族改为“包名等于 `crates/` 之下的路径以连字符连接”；[ADR-270](adr-270-provider-crate-family.md) 及更早 ADR 正文中的
  `crates/provider…` 路径按下表换算。包名、依赖图与依赖守卫不变。

## 背景

[ADR-270](adr-270-provider-crate-family.md) 把 provider 拆成 12 个 crate 后，`crates/` 的 25 个目录中
有 12 个以 `provider` 开头，与其余 13 个 crate 平铺在同一层，目录列表看不出它们是一个家族。
拆分的收益来自 crate 边界（各自的编译单元和测试二进制），与目录位置无关，因此可以只移动目录。

## 决策

1. **家族的 12 个 crate 位于 `crates/provider/<目录>`，包名等于 `crates/` 之下的路径以连字符连接。**
   `crates/provider/codex` 的包名是 `provider-codex`，新增的 provider crate 按同一规则命名。唯一的例外是
   运行时：它按 ADR-270 第 1 条保留包名 `provider`，目录为 `runtime`，不改名为 `provider-runtime`。
   现有包名都不变，`cargo -p <包名>`、`use provider_codex::…` 等按包名或 crate 名的引用也不变。

   | Package                     | 目录                               | 原目录                             |
   | --------------------------- | ---------------------------------- | ---------------------------------- |
   | `provider`                  | `crates/provider/runtime`          | `crates/provider`                  |
   | `provider-core`             | `crates/provider/core`             | `crates/provider-core`             |
   | `provider-native`           | `crates/provider/native`           | `crates/provider-native`           |
   | `provider-builtin`          | `crates/provider/builtin`          | `crates/provider-builtin`          |
   | `provider-antigravity`      | `crates/provider/antigravity`      | `crates/provider-antigravity`      |
   | `provider-claude`           | `crates/provider/claude`           | `crates/provider-claude`           |
   | `provider-codex`            | `crates/provider/codex`            | `crates/provider-codex`            |
   | `provider-deepseek-harness` | `crates/provider/deepseek-harness` | `crates/provider-deepseek-harness` |
   | `provider-minimax`          | `crates/provider/minimax`          | `crates/provider-minimax`          |
   | `provider-omp`              | `crates/provider/omp`              | `crates/provider-omp`              |
   | `provider-opencode`         | `crates/provider/opencode`         | `crates/provider-opencode`         |
   | `provider-pi`               | `crates/provider/pi`               | `crates/provider-pi`               |

2. **`crates/provider` 本身不是 package，根 `Cargo.toml` 逐个列出它的成员。** Cargo 对 glob 匹配到
   但没有 manifest 的目录报错，所以 `members` 保留 `crates/*`，用 `exclude = ["crates/provider"]`
   排除该目录，再显式列出 12 个成员。显式成员优先于 `exclude`；`crates/provider/*` 这样的 glob
   不算显式成员，写了也会被排除。新增 provider crate 时在 `members` 与 `[workspace.dependencies]`
   各加一行。漏写 `members` 不会报错：Cargo 只是不把它当作成员，`--workspace` 的检查和测试都会跳过它，
   所以依赖守卫（`bins/daemon/tests/dependencies.rs`）断言 `crates/provider` 下每个带 manifest 的目录
   都是 workspace 成员。
3. **ADR 的 `paths` 逐项换算，正文的叙述不改。** `crates/provider` 改为 `crates/provider/runtime`，
   `crates/provider-<x>` 改为 `crates/provider/<x>`，每个 ADR 约束的 crate 集合不变；本 ADR 约束
   `crates/provider` 整个目录。此前 ADR 与归档文档的正文记录的是当时的位置，叙述保持原样；只把会失效的
   链接目标改到新位置，并在 ADR-072 与 ADR-270 各加一行指向本决策。

## 后果与限制

行为、包名、feature、依赖图和 `Cargo.lock` 都不变。由目录推出包名时把路径用连字符连接：
`crates/provider/codex` 对应 `-p provider-codex`；只有 `crates/provider/runtime` 对应 `-p provider`。

跨 crate 读取夹具的路径随目录变化：daemon 与 `api` 的测试、`packages/protocol`、`packages/client`、
桌面与移动端测试读取 `crates/provider/<目录>` 下的 `tests/fixtures/` 或 `src/tests/fixtures/`，`verify.mjs`
按 `crates/provider/<目录>/tests/fixtures/` 把共享夹具的修改同时选给 TypeScript 包。从路径推断 crate 的
脚本随之调整：`seed-worktree-target.mjs` 在没有 manifest 的目录下再找一层，`rust_method_specs.py` 扫描
`crates/` 下任意深度的 `src/`，`paseo-focused-coverage.py` 把 `crates/provider/<目录>` 换算回包名；
读取 `cargo metadata` 的脚本不受影响。

移动前开出的分支合并时要跨越重命名：Git 的重命名检测会跟随已有文件，分支在旧目录新增的文件需要确认
落在新目录。归档文档和 `docs/plans/pr612-split` 的数据文件记录的是当时的路径，不改。
