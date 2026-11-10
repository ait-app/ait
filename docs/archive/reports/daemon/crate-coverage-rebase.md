# Daemon 测试分支 rebase 验证

日期：2026-10-03。最新 main：`be9347f83e039d9571af23b800313032346c7706`。测量源码：`c95ac040bdc391ee7cf51da9c0b13e119bdfea37`；
之后仅更新文档和覆盖率证据。

当前分支的原有测试与修复已 rebase 到上述 main。两处冲突为 Checkout 测试模块声明和
文档索引，均保留双方内容。与 rebase 前 `ae0b1e22` 比较，Rust 文件变化逐一匹配 main
新增内容；原 PR 的 Rust 实现及测试保留。[CI](../../../../.github/workflows/ci.yml) 与 main
一致，继续运行原有格式、Clippy、测试检查，覆盖率不作为 PR 合并门槛。

## 验证

普通与插桩全量运行均为 **1,751 passed、0 failed、3 ignored**。
通过 `cargo test --locked --offline --workspace`、`cargo build --locked --offline --workspace`、
`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`。
文档链接、修改文件格式、本地覆盖率脚本的 2 项测试和 `git diff --check` 通过。
普通构建使用 `CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target`，native 库路径与下面的测量命令相同。

## Test coverage

完整 13-crate workspace 行覆盖率：**94.5483%（48,595 / 51,397）**。
本次 rebase 引入 main 的 OpenCode 实现、relay crate 和相关协议改动，测量范围已扩大；
此前[12-crate 的 95% 测量](crate-coverage-95.md)仅对应历史源码，不代表当前所有 crate 均达到 95%。
未单独重测最新 main，没有相同范围的新基线，因此不计算可比百分比差值。

| crate      | 行覆盖率  | covered / total |
| ---------- | --------- | --------------- |
| api        | 92.3573%  | 1,861 / 2,015   |
| browser    | 97.3793%  | 706 / 725       |
| daemon     | 95.2991%  | 892 / 936       |
| domain     | 100.0000% | 121 / 121       |
| filesystem | 95.0171%  | 11,136 / 11,720 |
| metadata   | 95.0904%  | 7,360 / 7,740   |
| model      | 94.9591%  | 697 / 734       |
| protocol   | 100.0000% | 75 / 75         |
| provider   | 93.9359%  | 21,222 / 22,592 |
| relay      | 90.3017%  | 419 / 464       |
| schedule   | 97.8129%  | 805 / 823       |
| terminal   | 96.2202%  | 1,451 / 1,508   |
| voice      | 95.1646%  | 1,850 / 1,944   |

环境：macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、默认 features、默认文件过滤，
没有额外排除或新增 ignored 测试；不含 TypeScript 和 doctest 插桩。结果来自干净的单次全量运行。

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --locked --offline --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-crate-coverage/rebase-summary.json
```

[可审阅 JSON 证据](crate-coverage-rebase.json)包含逐 crate、逐文件行计数、测试结果、
源码指纹和原始报告哈希。HTML 位于 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`。
源码指纹为 `b2c68cbf42240fc855f67b606032563818e6dda4004b2f4a5c49e16caa53f166`。

3 项原有 ignored 依赖已安装/已认证的 Claude 或 Codex。真实 native 语音模型、
OpenCode 在线模型推理、Linux/Windows 运行和原生客户端未在本地验证；
新增主线模块的未覆盖路径仍应结合各自 Provider/relay 报告进行后续验证。
