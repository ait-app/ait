# Workspace Create Worktree 支持

- 日期：2026-09-28。
- 测量基线：`b707808cda1c70653e39a6dad3b4f4700c9d88ea` 加本 PR 修改；制品记录变更源码和被测 Rust 文件的 SHA-256，以及 Python fixture 的可执行位。
- 范围：`metadata`、`filesystem`、`api`、Codex 测试夹具和真实 server 进程回归。
- 设计：[ADR-060](../../../decisions/workspace/adr-060-workspace-create-worktree.md)。

## 行为

`workspace.create.request` 的 `source.kind = "worktree"` 调用既有 Worktrees 服务。
支持 `cwd` 或仅 `projectId`，新建分支和 checkout 已有分支，以及独立的 branchName 与
worktreeSlug。refName 是显式来源，baseBranch 是默认基准覆盖值。指定 title、预留
workspaceId 和 firstAgentContext 进入同一注册流程。

创建回执在 Git 副作用前预留身份；重复请求和重启后回放不创建额外 worktree，也不重跑
setup。使用另一个幂等键重复占用同一 workspaceId 会被拒绝。项目不存在、归档、非 Git
目录、未知分支等错误返回业务错误，注册失败保留既有 rollback 行为。

统一入口接入 main 的后台 metadata generation：首轮 prompt 和附件触发命名，显式
标题不建立自动命名资格，显式分支名不会被当成随机占位分支重命名。真实 Server 和离线
Provider 回归验证两个 worktree 入口的自动命名与显式分支保留。

App 已按 Paseo 结构发送来源参数，无需改 UI。Rust transport 继续声明
`creationLifecycle: false`，客户端沿用先创建 Workspace、再创建 Agent 的兼容路径。
直接在同一 RPC 中传 agent，以及 PR／Forge 来源 checkout，仍未支持。

## CI 启动修复

[首轮 PR CI](https://github.com/necokeine/ait/actions/runs/36335145045/job/108664476811)
在既有 Codex `failed_turn_exit_and_permission_requests_have_explicit_terminal_outcomes`
测试创建会话时返回 `Unavailable`。格式、Clippy、本次 worktree 与自动命名回归均通过。
启动错误映射没有保留 OS errno，因此临时可执行文件的 Linux 写入/启动竞争是根据
启动阶段与夹具实现作出的推断，不能把 `ETXTBSY` 视为已确认的原始错误。

Codex 单测和 server 进程测试改为通过各自的符号链接执行仓库内固定 fixture，脚本的
可执行位由 Git 跟踪。动态 transport 用例直接执行 Python，将脚本作为数据读取。
诊断用例通过链接旁的 `.cwd` 文件指定临时数据目录，避免改写共享脚本，并断言账户和
额度查询确实记录在该用例目录。生产代码、超时、失败断言和测试并发策略均保持原样。

## 验证

所有检查在独立 PR 工作区执行。使用默认 features、
macOS arm64、临时 Git 仓库和离线 Provider。端口与子进程测试在沙箱外执行。

定向测试：Codex **121 passed，0 failed，1 ignored**；server 进程回归
**59 passed，0 failed，0 ignored**，包括原失败用例、诊断目录隔离及动态 transport。

完整 workspace 测试由覆盖率运行执行：**1267 passed，0 failed，3 ignored**。
`cargo build --workspace --locked`、`cargo clippy --workspace --all-targets --locked -- -D warnings`
通过，无警告。当前 workspace 没有 doctest 用例。
Rust 格式、文档格式及 `git diff --check` 通过。

PR 已整合 main 的旧运行时清理，ADR 编号调整为 060。清理没有修改本次涉及的 server
源码；本报告和制品重新测量精简后的完整 workspace，仅包含 daemon 与 11 个 server crate。

所有 Cargo 命令使用 `CARGO_TARGET_DIR=/Users/necokeine/Documents/ait/target`：

```sh
cargo test -p provider local::codex:: -- --test-threads=4
cargo test -p daemon --test process -- --test-threads=4
cargo build --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo llvm-cov --workspace --html -- --test-threads=4
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-workspace-create-checks/fix-coverage.json
```

未启用 3 项既有真实 Provider ignored 测试，没有新增跳过项。未运行桌面 UI、真实付费
Provider 或远程 Forge 验收；Linux/Windows 未在本地验证，Linux 由 PR CI 执行。

## Test coverage

全 workspace 行覆盖率 **91.60%（35,725 / 39,000）**，相对
[当前架构](../../../architecture/README.md) 的 91.56%
变化约 **+0.04 个百分点**（差值按未舍入原始值计算）。两次均为 macOS arm64、默认 features
和当前 12 个 server 包；部分进程路径的覆盖也会随运行时行为波动。

| 范围                               | 已覆盖 / 总行数 | 行覆盖率 |
| ---------------------------------- | --------------- | -------- |
| Workspace                          | 35,725 / 39,000 | 91.60%   |
| metadata                           | 6,168 / 6,866   | 89.83%   |
| filesystem                         | 7,872 / 8,932   | 88.13%   |
| api                                | 1,110 / 1,156   | 96.02%   |
| daemon                             | 838 / 887       | 94.48%   |
| provider                           | 15,247 / 16,414 | 92.89%   |
| 新增 Worktree provisioning adapter | 81 / 88         | 92.05%   |

[可审阅 JSON 制品](workspace-create-worktree-coverage.json) 提供逐文件 SHA-256、覆盖行数、
完整 crate 汇总、测量命令、基线和变更测试源码指纹，可与提交内容核对。
HTML 已生成于 `/Users/necokeine/Documents/ait/target/llvm-cov/html/index.html`，共享产物以
仓库中的 JSON 为准。

无显式文件排除。测试文件按 cargo-llvm-cov 默认规则处理；`test_support.rs` 仍在测量范围内，
本次覆盖 31 / 31 行。Python fixture 不计入 Rust 行覆盖率，doctest 不计入默认插桩测量。
新增 adapter 未覆盖部分主要是锁损坏、registry/I/O/rollback 等少数错误映射；底层注册
失败回滚本身已有服务测试。后续扩展这些错误行为时应补 adapter 层的失败注入测试。
