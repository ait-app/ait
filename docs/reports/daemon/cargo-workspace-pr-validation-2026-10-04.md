# Cargo workspace 依赖整理与警告清理：PR 验证

源码提交：`7c1f6eba4eeab2adb32193de765b55a34d89b0a9`（包含本分支此前的 Cargo 整理提交 `88f44652`）。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。命令均使用默认 features、锁定依赖和离线模式。

## 变更范围

- 将外部依赖版本和可共享 features 放入根 `workspace.dependencies`，注册所有 `crates/` 包，统一成员清单的 `name.workspace = true` 写法。
- 清除 Rust 构建和 Clippy 警告：移除实际未使用的依赖、调整测试专用依赖，并修正冗余限定路径。集成测试目标中显式导入包级依赖，以满足 `unused_crate_dependencies`，未使用 lint `allow`。

## 构建、测试与静态检查

| 命令 | 结果 |
| --- | --- |
| `cargo build --workspace --all-targets --locked --offline` | 通过，0 warning |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 通过 |
| `cargo fmt --all --check`、`git diff --check` | 通过 |
| `cargo test --workspace --locked --offline` | 1,769 passed，0 failed，3 ignored |

完整测试在允许本机监听的环境中运行。沙箱内首次运行的 API 监听测试因 `Operation not permitted` 失败；解除该环境限制后全部通过。3 项 ignored 用例需要本机 Claude/Codex CLI 或认证，其中两项会发起真实模型请求。

## Test coverage

`cargo llvm-cov --workspace --html --locked --offline -- --test-threads=1` 插桩运行：1,769 passed，0 failed，3 ignored。`cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-pr-coverage-summary.json` 提取行统计。

| 范围 | 已覆盖 / 总行数 | 行覆盖率 |
| --- | ---: | ---: |
| Rust workspace | 49,024 / 51,879 | **94.50%** |
| daemon | 918 / 963 | 95.33% |
| api | 1,945 / 2,101 | 92.58% |
| filesystem | 11,143 / 11,735 | 94.96% |
| metadata | 7,523 / 7,938 | 94.77% |
| provider | 21,301 / 22,675 | 93.94% |

[可审查的覆盖率产物](cargo-workspace-pr-coverage-2026-10-04.json)包含所有 13 个 workspace 包和 336 个源文件的行统计，文件路径均相对仓库根目录。范围为上述源码提交的 Rust workspace、默认 features，使用工具默认过滤，无额外文件排除；不包括 TypeScript、Kotlin 或 doctest 插桩。本地 HTML 报告位于 `target/llvm-cov/html/index.html`。

[2026-10-03 的历史 workspace 测量](repository-audit-2026-10-03.md)为 43,974 / 47,707（92.18%），同平台、工具链和默认 features；本次高约 2.32 个百分点。两次之间还有其他源码变化，差异不能单独归因于本 PR。

未覆盖较多的文件包括 `crates/provider/src/local/opencode/http.rs`（112 行）、`crates/metadata/src/rpc/directory.rs`（96 行）、`crates/filesystem/src/local/checkout.rs`（92 行）。后续应针对这些路径的错误和恢复分支补充测试；真实 Provider 请求仍须在有认证的环境中验证。覆盖率首次并行插桩运行时有 daemon 进程测试超时，串行重跑全部通过。
