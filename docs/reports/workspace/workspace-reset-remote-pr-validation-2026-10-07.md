# 工作区重置同步远端分支：PR 验证

源码提交：`c4f340f72dbf9dccf79fb53e2e1cea9e8448aa38`；基于 main `c2a55fbe5817647eb9ac3ec7c53695df1bf885be`。平台：macOS arm64；Rust 1.98.1；cargo-llvm-cov 0.8.4。Cargo 使用锁定依赖、默认 features、离线模式和 `--test-threads=1`。覆盖率运行链接本机缓存的 sherpa-onnx v1.13.8 静态库。

## 行为与边界

本地工作区恢复初始分支并重置到 origin 的最新默认分支后，若 origin 存在初始分支同名引用，执行一次指定目标引用的 `--force-with-lease` 推送，将远端重置到相同提交。远端分支不存在时跳过推送。Lease 使用实际查询到的远端提交，避免残留跟踪引用、并发更新或删除导致错误覆盖。

推送失败返回明确的部分完成提示：本地已重置，远端未完成；本地不回滚，持久化分支字段沿用完整成功后更新的规则。九种语言的确认提示已同步，决策记录见 [ADR-092](../../decisions/workspace/adr-092-reset-same-named-remote-branch.md)。

## 测试与静态验证

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace --locked --offline -- --test-threads=1` | 1,894 passed，0 failed，7 ignored |
| `cargo llvm-cov --workspace --html --locked --offline -- --test-threads=1` | 1,894 passed，0 failed，7 ignored |
| `cargo build --workspace --locked --offline` | 通过，无编译警告 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 通过 |
| `cargo fmt --all --check`、`git diff --check` | 通过 |
| `npm run test --workspace=@ait/mobile -- src/i18n/resources.test.ts` | 36 passed，0 failed |
| `oxfmt --check`、`oxlint`（语言资源文件） | 通过，0 lint warning/error |
| `npm run check:docs` | 通过 |

七项 ignored 沿用原配置，要求本机安装或认证 AGY、Claude、Codex、DSH 或 OpenCode；没有增加忽略项。完整测试允许启动本机 HTTP/WebSocket 监听和子进程，未执行真实模型请求。

## Test coverage

| 范围 | 已覆盖 / 总行数 | 行覆盖率 |
| --- | ---: | ---: |
| Rust workspace | 53,569 / 56,802 | **94.3083%** |
| filesystem | 11,366 / 11,988 | **94.8115%** |
| 改动的 local checkout 文件 | 1,515 / 1,622 | 93.4032% |

测量范围为上述源码提交的完整 Rust workspace、默认 features，使用 cargo-llvm-cov 默认文件过滤，没有额外排除文件或筛选测试。Doctests 由普通完整测试执行，未纳入覆盖率插桩；原生 C/C++ 依赖未插桩。未验证 Linux 和 Windows。

强制推送调用执行 12 次，推送失败错误包装执行 3 次，最终本地 hard reset 执行 23 次。回归覆盖 main/master、已有或已检出的初始分支、工作分支改名、跟踪引用缺失或残留、非快进推送、拒绝后重试、一次推送且不附带标签，以及查询后远端更新或删除。

[可审阅覆盖率证据](workspace-reset-remote-pr-coverage-2026-10-07.json)记录精确命令（含 `SHERPA_ONNX_LIB_DIR`）、源码提交、源码哈希、workspace 与各 crate 行统计、改动文件与行为区域执行次数、默认忽略项和测量限制。HTML 报告生成于 `target/llvm-cov/html/index.html`；本地 HTML 路径不作为共享产物。

与[同日历史基线](../providers/paseo-concurrency-coverage-2026-10-07.json)相比，workspace 从 94.3074% 变为 94.3083%（+0.0009 个百分点），filesystem 从 94.8081% 变为 94.8115%（+0.0034 个百分点）。该基线的 Rust 源码与本次 main 基准一致，平台、工具链、features 和测试并发也一致；没有重新测量基线，运行时调度差异可能影响其他模块执行数，不能把 workspace 差异全部归因于本次修改。

本地 Git 校验、fetch/reset 的错误传播，以及进程超时和输出预算路径仍未全部作故障注入。远端推送拒绝通过隔离的 bare 仓库模拟，未访问真实托管仓库验证分支保护或凭据错误。后续由 Linux CI 和对应平台补充验证；需要直接覆盖其余错误路径时增加可控 Git 失败注入。
