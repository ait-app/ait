# Crate 边界整合到最新 main 的验证

日期：2026-10-07。Rebase 基线为 `0c137cd7ca56ea734d1cfd846c9338a460898893`。
测量源码为 `b7d1478697beeeb157232a23ab62ef1576e24267` 加本报告所在提交的 Rust 适配；
[验证证据](crate-boundaries-main-rebase-2026-10-07.json) 保存修改文件与全部被测生产文件的 SHA-256。

## 整合结果

保留 main 的消息分块、文件上传确认、可空创建 Agent、Provider 自选辅助模型及 DSH/OpenCode
原生通道。辅助生成能力和模型选择统一使用 summary 接口及 model 类型，功能 crate 间依赖
仍被守卫拒绝。API 上传测试安装完整 filesystem 服务；归档容量测试直接构造内部请求状态。
本分支的五个 ADR 改为 100–104，保留 main 的 ADR-097、098 和 099，并更新全部文档引用。

`cargo test --workspace -- --test-threads=4`：1975 通过、0 失败、15 忽略。
覆盖率运行的测试结果相同。`cargo build --workspace`、
`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、
`git diff --check` 和 `npm run check:docs` 均通过。

初次 `cargo test --workspace` 使用默认并发时，一个目录订阅进程测试等待事件超时。
`RUST_BACKTRACE=1 cargo test -p daemon --test process directory_sync -- --nocapture`
单独运行的两项测试通过；将完整测试和覆盖率运行限制为四个测试线程后也全部通过。
没有为绕过超时修改生产代码；默认并发下的偶发超时仍需后续复现和诊断。

## Test coverage

命令：`cargo llvm-cov --workspace --html -- --test-threads=4`。
范围为完整 Rust workspace、默认 features、macOS arm64，无额外文件排除。
15 项需要安装的原生 CLI、已有会话或认证的测试按默认配置忽略；Linux/Windows 未验证。
逐项忽略原因、命令、逐 crate/文件行数及源码摘要保存在
[可评审的 JSON 证据](crate-boundaries-main-rebase-2026-10-07.json)。
本地 HTML 位于 `target/llvm-cov/html/index.html`。

| 范围                | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------- | -------: | --------------: |
| `workspace`         |   94.09% |   55684 / 59179 |
| `bins/daemon`       |   95.52% |       939 / 983 |
| `crates/api`        |   93.30% |     2146 / 2300 |
| `crates/browser`    |   97.54% |       715 / 733 |
| `crates/domain`     |  100.00% |       121 / 121 |
| `crates/filesystem` |   94.87% |   11564 / 12189 |
| `crates/metadata`   |   94.29% |     6089 / 6458 |
| `crates/model`      |   96.37% |     2495 / 2589 |
| `crates/protocol`   |  100.00% |         63 / 63 |
| `crates/provider`   |   93.16% |   26909 / 28886 |
| `crates/relay`      |   90.95% |       422 / 464 |
| `crates/schedule`   |   97.52% |       826 / 847 |
| `crates/terminal`   |   96.46% |     1527 / 1583 |
| `crates/voice`      |   95.16% |     1868 / 1963 |

同平台、默认 features 的 rebase 前 workspace 结果为 94.45%（54897/58122），
本次为 94.09%（55684/59179），减少约 0.36 个百分点。main 增加了生产代码和默认忽略用例；
[原报告](../workspace/filesystem-model-independence-2026-10-07.md) 保留旧源码的测量结果。

API 方法声明与组装适配器均为 63/63，Provider 装配为 58/58，辅助模型选择为 44/44。
剩余未覆盖行为包括 DSH 辅助通道（57/103）、OpenCode 辅助通道（62/198）、
部分消息分块非法输入/发送失败分支、Runtime 排队 admission 取消竞态与候选推理等级覆盖。
后续修改这些行为时应补充定向错误/竞态测试，并在具备所需原生环境时运行相应 ignored 测试。
真实远端 GitHub、安装/认证 Provider 和其他平台不在本次验证范围内。
