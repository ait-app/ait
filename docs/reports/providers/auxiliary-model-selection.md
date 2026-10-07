# Provider 辅助模型选择验证

范围：Provider 能力声明、adapter 自选小模型、用户覆盖、Codex/Claude 装配；不含 OpenCode/DSH 新通道或 Antigravity 变更。

## Test coverage

测量源码：基于 `f83d9579`，Git tree `2f37507de76c78c558ea7997f1374c189e2f856b`（加入本报告前的已暂存实现）。默认 features，Linux x86_64，未排除源码；10 项需本机 CLI 的 ignored 测试保持跳过，未验证 macOS/Windows。

命令：

```sh
CARGO_TARGET_DIR=/home/lonnet/Developers/ait/target \
XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config \
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata \
cargo llvm-cov --workspace --html
```

本表是从本次 LLVM HTML index 提取的可审查覆盖率摘要。HTML 生成于 `target/llvm-cov/html/index.html`，完整 HTML 未上传。

| 范围 / 文件 | 覆盖行 / 总行 | 行覆盖率 |
|---|---:|---:|
| Workspace | 54751 / 57980 | 94.43% |
| Provider crate | 26262 / 27973 | 93.88% |
| provider/src/composition.rs | 54 / 54 | 100% |
| provider/src/local/metadata_model.rs | 35 / 35 | 100% |
| provider/src/ports/agent_session.rs | 223 / 229 | 97.38% |
| provider/src/service/metadata_generation.rs | 75 / 77 | 97.40% |
| provider/src/service/metadata_generation/candidates.rs | 79 / 80 | 98.75% |

无相同基线提交和测量环境下的覆盖率结果，故不报告可比增减。未覆盖部分包括部分默认 port 行为与候选覆盖分支；后续实际通道 PR 需补充安装 CLI 的验证。

## 测试执行

定向 metadata 测试 28 项通过；全 workspace 1924 项通过、10 项 ignored。`cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。测试使用 `XDG_CONFIG_HOME=/tmp/ait-pr-isolated-config` 隔离本机 Git hooks；文档链接检查通过。

全量测试发现并修正一个依赖前台模型作为辅助模型的并发测试夹具：现在显式配置其辅助模型，仍验证首次用户输入失败后不自动重放。
