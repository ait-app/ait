# Provider 装配边界验证

日期：2026-10-06。源码 revision：`f7e0ddca886a1540997253743c4740e161282a21`。
基线：PR #196 修订前的 `564d15b95ffe981d8f616c919f770f62d00d63c6`；平台为 macOS arm64。
测量在提交源码前完成，测量与提交的 Rust 文件相同，聚合 SHA-256 为
`73666ba1f77bc6131f553686a1afc718b9d8d73a49ba5831da7db8afa8f72107`。逐文件哈希见[覆盖率工件](provider-composition-coverage.json)。

## 结果与范围

`provider::Providers` 统一配置内置 adapter、元数据生成与前台注册。
daemon 不再引用具体 provider 类型、维护注册列表或读取 adapter 的启动覆盖变量。
`local` 模块已设为私有；Codex/Claude 的选择依据是结构化辅助生成能力。
边界与兼容约束见 [ADR-089](../../decisions/providers/adr-089-provider-owned-composition.md)。

| 验证 | 结果 |
| --- | --- |
| 新装配测试 | 4 通过，0 失败 |
| daemon 启停回归 | 2 通过，0 失败 |
| 元数据生成端到端回归 | 2 通过，0 失败 |
| Rust workspace 完整测试 | 1861 通过，0 失败，7 ignored |
| workspace 覆盖率插桩测试 | 1861 通过，0 失败，7 ignored |
| `dsh` 简称修订后的插桩回归 | 装配 4、创建 5、进程 1 项通过，0 失败 |
| workspace 构建、严格 Clippy、Rust 格式 | 通过 |
| 文档链接与 diff 格式检查 | 通过 |

装配测试验证完整 catalog、重复注册、构造与注册不创建存储、显式错误路径不回退到
主机安装程序、DSH native/ACP 能力，以及两个客户端的结构化辅助生成与关闭。
离线夹具验证 Codex ephemeral session 和 Claude `--no-session-persistence`。
端到端回归覆盖 workspace 名称、Agent 标题、commit 文案和 managed worktree 命名。
本次没有前端代码变更；没有重新执行认证模型请求。

```bash
cargo test -p provider composition::
cargo test -p daemon host::tests::
cargo test -p daemon --test process metadata_generation -- --test-threads=1
cargo test --workspace -- --test-threads=1
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm run check:docs
git diff --check
```

## Test coverage

| 测量范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Cargo workspace | 51896 / 54993 | **94.37%** |
| `provider` | 23936 / 25523 | **93.78%** |
| `daemon` | 898 / 941 | **95.43%** |
| Provider 装配生产代码 | 51 / 51 | **100.00%** |

使用 Rust 1.98.1、LLVM 22.1.8 和 cargo-llvm-cov 0.8.4，在 macOS 27.0.1 arm64 上测量。
整个 Cargo workspace 使用默认 features、工具默认源文件过滤，没有额外排除。
完整验证后按命名要求把 DeepSeek Harness 的简称改为 `dsh`，以 `--no-clean` 重跑受影响的
装配、订阅创建和 daemon 进程测试，再合并生成当前源码的报告；这些额外运行单独计数。
与相同平台、命令和 features 的 `564d15b9` 基线相比，workspace 变化
+0.0074 个百分点，provider 变化 +0.0164 个百分点，
daemon 变化 +0.0553 个百分点。新增装配模块没有单独的旧版基线。

```bash
cargo llvm-cov --workspace --html --no-fail-fast -- --test-threads=1
cargo llvm-cov --no-clean -p provider -- composition::
cargo llvm-cov --no-clean -p provider -- service::agent_execution::tests::subscriptions::creation
cargo llvm-cov --no-clean -p daemon --test process -- deepseek_harness --test-threads=1
cargo llvm-cov report --html
cargo llvm-cov report --json --summary-only --output-path target/provider-composition-validation/coverage-summary.json
cargo llvm-cov report --lcov --output-path target/provider-composition-validation/coverage.lcov
```

可共享的[覆盖率工件](provider-composition-coverage.json)包含 workspace/逐 crate 计数、
装配模块逐文件指标与未覆盖行、源码哈希、测试数量和基线差值。
本机 HTML 位于 `target/llvm-cov/html/index.html`。
未插桩 doctest 或 TypeScript/UI；七项认证或显式安装原生 CLI 的测试按默认 ignored 跳过。
Linux 和 Windows 未实机运行。新增装配生产代码没有未覆盖行；各 adapter 原有的异常 I/O、
在线工具与平台路径分支仍需对应夹具和实机验证，本次没有扩展这些能力。
