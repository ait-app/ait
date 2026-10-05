# 工作区重置复用同名分支：PR 验证

源码提交：`7b31f225d662cb9ae4d1b9ee2c95fd44ca37f33e`；基于 main `18631193f52e045407e2b43382d3de5df465809b`。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。Cargo 使用默认 features 和离线模式，`SHERPA_ONNX_LIB_DIR` 指向本机缓存的 sherpa-onnx v1.13.8 静态库；完整测试使用 `--test-threads=4`。

## 变更与回归场景

工作区分支改名后，创建时的同名本地分支可能已经重新存在。原逻辑直接用 `git branch -m` 恢复名称，因此在同名分支存在时失败。

重置现在先 fetch `origin` 的最新默认分支；若初始分支已存在，则强制检出该分支，再执行 `git reset --hard`。不存在时仍将当前分支改名。回归测试覆盖 main/master 在重置前推进、初始分支已存在或已检出、脏文件丢弃、未跟踪文件保留，以及复用分支时保留原分支引用。同名分支被其他 worktree 检出时，两个 worktree 的 HEAD 和脏文件均保持原样。远端查询失败也保持当前分支和 HEAD。

## 验证结果

| 命令                                                              | 结果                                    |
| ----------------------------------------------------------------- | --------------------------------------- |
| `cargo test --workspace --offline -- --test-threads=4`            | 通过；1,803 passed，0 failed，3 ignored |
| `cargo llvm-cov --workspace --html --offline -- --test-threads=4` | 通过；1,803 passed，0 failed，3 ignored |
| `cargo build --workspace --offline`                               | 通过，无编译警告                        |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | 通过                                    |
| `cargo fmt --all --check`、`git diff --check`                     | 通过                                    |

首次默认并发的完整测试在目录同步集成测试等待 WebSocket 消息时超时。该测试单独复核通过；后续降低并发的完整测试及覆盖率运行都通过。三个忽略测试沿用原配置：一个要求本机安装 Claude CLI，另两个要求真实 Claude/Codex 认证并会发送模型请求。

## Test coverage

| 范围                       | 已覆盖 / 总行数 |   行覆盖率 |
| -------------------------- | --------------: | ---------: |
| Rust workspace             | 49,428 / 52,323 | **94.47%** |
| filesystem                 | 11,338 / 11,961 | **94.79%** |
| 改动的 local checkout 文件 |   1,488 / 1,595 |     93.29% |

测量范围为上述源码提交的完整 Rust workspace、默认 features，使用 cargo-llvm-cov 默认文件过滤，没有额外排除文件或筛选测试；未验证 Linux 和 Windows。已有分支检出语句执行 3 次，分支不存在时的改名语句执行 7 次，最终 hard reset 语句执行 10 次。

[可审查覆盖率产物](workspace-reset-existing-branch-pr-coverage-2026-10-05.json)记录精确命令、源码提交、workspace 与各 crate 行统计、改动文件及行执行次数、忽略测试和重试情况。HTML 报告生成于 `target/llvm-cov/html/index.html`；测试结果与覆盖率分开统计。

与[同平台、工具链的历史测量](workspace-reset-path-pr-coverage-2026-10-05.json)相比，workspace 从 94.4764% 变为 94.4671%，变化 -0.0093 个百分点；filesystem 从 94.8323% 变为 94.7914%，变化 -0.0409 个百分点。包版本和测试并发不同，运行时调度也可能影响覆盖，不能把全部差异归因于本修复。

重置的 fetch、引用查询、改名及最终 hard reset 的错误传播仍未覆盖；已有分支检出失败已由其他 worktree 占用测试覆盖。后续可用可控的 Git 失败注入补齐其余错误路径；真实认证 Provider 测试需在具备安装与认证的主机单独执行。
