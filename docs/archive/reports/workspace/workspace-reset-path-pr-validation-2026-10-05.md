# 工作区重置路径校验：PR 验证

源码提交：`bfd87a1618accc501346cb44d0ced4984ddb20bb`；基于 main `3b10f32612977b373ac76f28c09f56a7fe8b1758`。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。Cargo 命令使用默认 features 和离线模式，`SHERPA_ONNX_LIB_DIR` 指向本机缓存的 sherpa-onnx v1.13.8 静态库。

## 变更与回归场景

持久化记录中的工作区路径可能以 `/` 结尾，客户端会移除末尾分隔符。重置请求曾直接比较字符串，导致同一目录被误报为 `Workspace identity or initial branch mismatch`。

重置前的身份校验与重置后的分支记录更新现在都通过 Rust `Path` 比较路径组件。回归测试覆盖末尾分隔符、重复分隔符和 `/.`，验证重置恢复初始分支、丢弃已跟踪文件改动并同步持久化分支。不同目录（包括同一 worktree 的子目录）、错误初始分支、已归档及非托管工作区仍会被拒绝。

## 验证结果

| 命令                                                              | 结果                                    |
| ----------------------------------------------------------------- | --------------------------------------- |
| `cargo llvm-cov --workspace --html --offline`                     | 通过；1,800 passed，0 failed，3 ignored |
| `cargo build --workspace --offline`                               | 通过                                    |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | 通过                                    |
| `cargo fmt --all -- --check`、`git diff --check`                  | 通过                                    |

首次在受限沙箱运行时，API 集成测试因无法绑定本机端口而失败。上表及覆盖率数据来自允许本机测试服务的最终完整运行。三个忽略测试沿用原配置：一个要求本机安装 Claude CLI，另两个要求真实 Claude/Codex 认证并会发送模型请求。

## Test coverage

| 范围                         | 已覆盖 / 总行数 |   行覆盖率 |
| ---------------------------- | --------------: | ---------: |
| Rust workspace               | 49,431 / 52,321 | **94.48%** |
| filesystem                   | 11,341 / 11,959 | **94.83%** |
| 改动的 checkout service 文件 |       158 / 169 |     93.49% |

测量范围为上述源码提交的完整 Rust workspace、默认 features，使用 cargo-llvm-cov 默认文件过滤，没有额外排除文件或筛选测试；未验证 Linux 和 Windows。两处改动的路径比较分别执行 10 次和 5 次。

[可审查覆盖率产物](workspace-reset-path-pr-coverage-2026-10-05.json)记录精确命令、源码提交、workspace 与各 crate 行统计、改动文件覆盖率及忽略测试。HTML 报告生成于 `target/llvm-cov/html/index.html`。测试成功数量与行覆盖率分别统计。

与[同平台、工具链及修复前 Rust 源码的历史测量](workspace-git-actions-pr-coverage-2026-10-05.json)相比，workspace 从 94.4688% 增至 94.4764%，提高约 0.0076 个百分点；filesystem 从 94.8073% 增至 94.8323%，提高约 0.0250 个百分点。两次测量的行总数相同，workspace 包版本从 0.0.17 更新为 0.0.18；其他运行时测试的覆盖也可能随调度变化，因此不能把 workspace 的全部差异归因于本修复。

现有的重置期间记录变更和 registry 存储错误分支仍未覆盖，后续可用会变更或失败的 mock registry 补齐。真实认证 Provider 测试需在具备安装与认证的主机单独执行。
