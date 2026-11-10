# 用 Ait 开发 Ait：产品缺口

本仓库已成为 Ait 的第一个客户：[`ait.json`](../../ait.json) 在新 Worktree 中克隆 `target/`，
`verify` 脚本把改动文件映射到格式、lint、受影响测试和文档检查，CI 调用同一个
[`scripts/verify.mjs`](../../scripts/verify.mjs)。下列缺口来自这次接入，均已对照源码。

## 范围语义

| `--scope`        | 使用者       | Rust 测试范围             | UI 范围                |
| ---------------- | ------------ | ------------------------- | ---------------------- |
| `owners`（默认） | 本地         | 拥有改动文件的 crate      | 改动涉及的客户端或包   |
| `closure`        | Pull Request | 上述 crate 及其全部依赖方 | 只要涉及 UI 就运行全部 |
| `all`            | 推送 main    | 整个 workspace            | 全部                   |

各范围的 clippy 都覆盖依赖方，只编译不运行测试，因此本地仍遵守[仓库约定](../../AGENTS.md)的测试范围。

## 缺口

按对“任务到已验证合并”闭环的影响排序。

1. **验证结果不影响 PR 或合并。** 脚本运行已有记录（提交、是否有未提交改动、结果、耗时、有界输出），
   Agent 可以带参数启动并等待结束，见 [ADR-229](../decisions/workspace/adr-229-workspace-script-runs.md)。
   但记录只在 daemon 内存中，重启即丢失，也不关联 PR 状态或合并门禁。需要决定持久化位置以及是否、如何作为门禁。
2. **没有构建缓存原语。** `AIT_SOURCE_CHECKOUT_PATH` 总指向主检出，而主检出常常最旧。
   种子脚本只好自己遍历 `git worktree list`，比较工具链、卷和新旧，还要清除工作区 crate 的 fingerprint，
   否则 Cargo 可能把其他分支的产物当作最新。应在配置中声明缓存目录，由 daemon 选择来源并克隆；
   “来源兼容”的判定（工具链、卷、新旧）和失效规则依赖构建工具，需先决定 daemon 承担多少。
3. **配置来自来源检出而非分支。** 分支没有 `ait.json` 时，创建 Worktree 会把来源检出的配置复制为未跟踪文件，
   其引用的脚本可能不存在；该行为现已写入[操作文档](../operations/workspace-automation.md#新-worktree-中的配置来源)，
   但尚未决定是否改变：停止复制、仅复制未提交的本地配置，或在列表中标明配置来源。
4. **移动端停止脚本无效。** 脚本的 `terminalId` 只是逻辑标识，移动端用 `killTerminal(terminalId)` 停止脚本，
   对脚本进程无效。应改用 `workspace.script.stop.request`，或决定让脚本运行在真实终端中。

已关闭：脚本输出捕获与读取、运行记录与等待、脚本参数（[ADR-229](../decisions/workspace/adr-229-workspace-script-runs.md)），
归档时运行 `teardown`（[ADR-230](../decisions/workspace/adr-230-worktree-teardown.md)），
`ait.json` 操作文档与 `AIT_*` 环境变量（[Workspace 自动化](../operations/workspace-automation.md)）。
本仓库的 `ait.json` 仍把 `verify` 输出重定向到 `.tmp/verify.log`，以兼容尚未升级的 daemon；
升级后可改为 `"command": "node scripts/verify.mjs \"$@\"", "acceptsArgs": true`。

## 尚未验证

- `sandboxed-worktree` 下的 Agent 运行 `verify`：Cargo 和 npm 需要写入 Worktree 之外的缓存，可能被拒绝。

GitHub Actions 上的实际运行已确认：2026-11-10 推送 main（`7d110395`）的 CI 运行成功。
