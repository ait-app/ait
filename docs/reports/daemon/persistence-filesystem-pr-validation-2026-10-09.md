# Persistence 与 filesystem 能力边界：PR 验证

日期：2026-10-09。测量基于 `913170dcae8c11eb336ffaae0b99d7d576fed98d`，
全部改动的暂存 Git tree 为 `bda551a0a3945d0d41a151779edabccf3a2fc8ed`。
[JSON 证据](persistence-filesystem-pr-coverage-2026-10-09.json)记录源文件 SHA-256、精确行数、
忽略原因和变更文件的零命中行。后续新增验证文档及接入 main 的 CI 改动不改变测量的 Rust 源码。

## 验证结果

普通完整 workspace 测试：**2044 通过、0 失败、15 忽略**，包括 persistence 的 doctest。
插桩完整 workspace 测试：**2043 通过、0 失败、15 忽略**；覆盖率不插桩 doctest。
格式、完整构建、全目标 Clippy、文档链接与检查器测试通过；Rust 方法解析测试和覆盖率检查器测试通过。
客户端校验通过：172 个客户端方法对应 179 个 Rust 组件声明。

首轮默认并发普通测试中，目录同步进程用例 WebSocket 接收超时；该用例独立复跑通过，
最终四线程全量普通测试和单线程全量插桩测试通过，没有修改用例或生产代码。

边界实现见 [ADR-112](../../decisions/daemon/adr-112-persistence-crate.md) 与
[ADR-113](../../decisions/workspace/adr-113-filesystem-capability-groups.md)。

## Test coverage

范围：全部 13 个 Rust workspace package，默认 features、默认文件过滤，无额外文件排除；
macOS arm64、rustc 1.98.1、cargo-llvm-cov 0.8.4。15 个原有用例依赖本机原生 Provider 安装、
会话、环境变量或认证，按默认配置忽略，逐项原因见 JSON。Linux/Windows 和 TypeScript 覆盖率未测量。

| 范围 | 行覆盖率 | 已覆盖 / 总行数 |
| --- | ---: | ---: |
| workspace | 94.1951% | 57118 / 60638 |
| api | 94.2895% | 2130 / 2259 |
| browser | 97.5443% | 715 / 733 |
| daemon | 94.7969% | 1330 / 1403 |
| domain | 100.0000% | 549 / 549 |
| filesystem | 94.8667% | 11569 / 12195 |
| metadata | 94.1268% | 5449 / 5789 |
| model | 96.2154% | 1322 / 1374 |
| persistence | 95.7877% | 1751 / 1828 |
| provider | 93.2790% | 27674 / 29668 |
| relay | 91.8489% | 462 / 503 |
| schedule | 97.5980% | 772 / 791 |
| terminal | 96.4624% | 1527 / 1583 |
| voice | 95.1605% | 1868 / 1963 |

相同工具、默认 features 与过滤规则下，workspace 聚合较上一份 Domain/Server 报告
变化 **+0.0154 个百分点**；file 更名为 persistence，启动配置与 identity 等迁入 daemon，
不直接比较这些 crate 的百分比变化。

JSON 中的 `changed_files_zero_hit_lines` 保存变更文件未覆盖的可执行行；原有 Provider 分支、
进程/文件 I/O 错误路径仍有缺口。后续修改这些行为时应补充可控故障测试，并在原生 Provider
可用环境及 Linux/Windows CI 复查。HTML 已生成到 `target/llvm-cov/html/index.html`，
原始 profile、HTML、日志和完整 LLVM 导出留在忽略的 target 或临时目录；提交的 JSON 为可审阅的精简证据。

## 原样保留的未接入内容

按用户选择保留 `storage/summary_config.rs`、其 tests.rs 和摘要 ADR-109。
两份 Rust 文件没有在 persistence 的模块树中声明，且引用的 `model::summary::SummaryConfiguration`
不存在，因此本次编译、测试和覆盖率不验证它们。该 ADR 描述的是未接入方案，编号也与已有
诊断 ADR-109 重复；当前实际摘要接口与 daemon 配置适配位置未由这些文件改变。

补充源码位置检查发现 Paseo 历史快照有 6 个失效路径，均已存在于基线 HEAD，见 JSON。
当前组件声明/客户端方法校验通过；历史快照位置需要在后续重新生成审计记录时清理。

## 精确命令

```sh
cargo fmt --all --check
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --test-threads=4
cargo llvm-cov --workspace --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path target/persistence-filesystem-coverage-raw.json
cargo llvm-cov report --lcov --output-path target/persistence-filesystem-coverage.lcov
node scripts/check-docs.mjs
node --test scripts/check-docs.test.mjs
python3 scripts/rust_method_specs_test.py
python3 scripts/check-crate-coverage.test.py
python3 scripts/check-paseo-client-methods.py
git diff HEAD --check
```
