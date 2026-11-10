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

1. **脚本输出被丢弃。** `start_script` 把 stdout 和 stderr 接到空设备，`terminalId` 也不对应真实终端，
   Agent 只能得到退出码。当前在 `ait.json` 里重定向到 `.tmp/verify.log` 绕过。应像 setup 一样捕获输出并提供读取入口。
2. **没有“验证结果”这一概念。** 一次运行没有记录，不关联提交或 diff，也不影响 PR 状态或合并。
   Agent 只能轮询 `list_workspace_scripts`。应记录提交、结果、耗时，并让 Agent 能等待运行结束。
3. **脚本不能传参。** Agent 无法请求 `verify --scope all`，只能为每种组合增加一个脚本条目。
4. **没有构建缓存原语。** `PASEO_SOURCE_CHECKOUT_PATH` 总指向主检出，而主检出常常最旧。
   种子脚本只好自己遍历 `git worktree list`，比较工具链、卷和新旧，还要清除工作区 crate 的 fingerprint，否则 Cargo 可能把其他分支的产物当作最新。应在配置中声明缓存目录，由 daemon 选择来源并克隆。
5. **配置来自来源检出而非分支。** `seed_config` 会把 `ait.json` 复制到不含它的分支上，其引用的脚本可能不存在，
   所以 setup 命令必须先判断文件。应让配置与分支版本一致，或明确缺失时的行为。
6. **`teardown` 只被校验。** 项目配置接受该字段，daemon 没有执行它，归档 Worktree 时无法运行清理。
7. **`ait.json` 没有操作文档。** 字段、环境变量和生命周期只能从 `crates/metadata/src/automation/local.rs` 读出，
   且环境变量仍只有 `PASEO_*` 名称。

## 尚未验证

- `sandboxed-worktree` 下的 Agent 运行 `verify`：Cargo 和 npm 需要写入 Worktree 之外的缓存，可能被拒绝。
- GitHub Actions 上的实际运行：改动的工作流需要推送后才能观察。
