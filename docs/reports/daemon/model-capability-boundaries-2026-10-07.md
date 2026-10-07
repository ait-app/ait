# 共享 model 与功能 crate 边界验证

日期：2026-10-07。实现范围对应 ADR-101、ADR-102 和 ADR-103，尚未包含 filesystem 去除 metadata 依赖的后续修改。
测量源码为父提交 `5467bc48188df86ec9180e93d0f709eb00e7acdd` 加本报告所在提交的 Rust 改动；JSON 保存暂存差异摘要和逐文件 SHA-256，明确识别测量版本。

## 验证结果

`cargo test --workspace`：1931 通过、0 失败、10 忽略。覆盖率运行同样为 1931 通过、0 失败、10 忽略。
忽略项均为需要安装或认证原生 Provider 的测试；本轮没有显式跳过测试。

以下检查通过：`cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo fmt --all --check`、`git diff --check`、`npm run check:docs`。
沙箱内首次测试因禁止监听本地端口失败；上述最终结果来自允许本地端口和 PTY 的执行环境。

## Test coverage

命令：`cargo llvm-cov --workspace --html`。测量 macOS arm64 上整个 Rust workspace 的默认 features，
没有额外文件排除；Windows/Linux 未测量。HTML 生成于 `target/llvm-cov/html/index.html`。
可评审证据为同提交的 [逐文件覆盖率与源码摘要](model-capability-boundaries-2026-10-07.json)。没有同口径的迁移前基线，因此不报告覆盖率增量。

| 测量范围            | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------- | -------: | --------------: |
| workspace           |   94.43% |   54841 / 58076 |
| `crates/model`      |   96.06% |     2270 / 2363 |
| `crates/provider`   |   93.89% |   26284 / 27995 |
| `crates/terminal`   |   96.46% |     1500 / 1555 |
| `crates/metadata`   |   94.40% |     6287 / 6660 |
| `crates/api`        |   92.70% |     1993 / 2150 |
| `crates/filesystem` |   94.86% |   11555 / 12181 |
| `bins/daemon`       |   95.51% |       937 / 981 |

API summary 适配器 51/51 行、metadata 协作适配器 37/37 行均被覆盖，
provider summary coordinator 为 75/77 行。尚未覆盖的部分主要包含原生 Provider 与异常/取消分支；
逐文件缺口可从 JSON 的 covered/total 查阅，具体未命中行可在 HTML 查看。
后续若改动这些行为，应补对应定向测试；已忽略的真实 Provider 用例需在配置好 CLI/认证的环境单独运行。
