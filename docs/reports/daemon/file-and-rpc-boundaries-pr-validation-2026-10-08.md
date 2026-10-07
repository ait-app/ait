# File、共享契约与 RPC 边界：PR 验证

日期：2026-10-08。测量源码：`a2aa8416c623a91c6e17b332acfc802a23705eb9`；完整测试在同一暂存源码快照上运行后提交。
[JSON 证据](file-and-rpc-boundaries-pr-coverage-2026-10-08.json) 保存逐 crate/文件行数、源码摘要、
全部命令、测试计数和忽略原因。随后合并 PR 的 rebase 前历史及 main 的界面修复，Rust/Cargo
源码未改变，指纹校验仍一致；这些合并提交不改变本报告的 Rust 测量范围。

## 结果

`file` 拥有单文件读写、原子替换、轮询观察、通用 FileRegistry、启动配置及具体持久化适配器。
model 保留共享记录、存储契约及创建协调，功能服务通过端口接受宿主注入。调用处直接引用
所属 crate，删除迁移转发模块；provider/schedule 仅在测试中依赖 file。relay 拥有控制 RPC
并依赖 model；metadata 声明九个始终可用的基础连接方法，API 保留传输、鉴权和跨能力协调。
完整宿主仍有 179 个实现方法、180 个可协商能力，精简 host 保留 12 个方法。
边界决策见 ADR-100 至 ADR-108，由[当前架构](../../architecture/README.md)链接各项决策。

普通 workspace 测试：**1998 通过、0 失败、15 忽略**，其中包含 file 的 1 个 doctest。
插桩 workspace 测试：**1997 通过、0 失败、15 忽略**；该命令不插桩 doctest。
完整构建、workspace Clippy、格式与文档链接检查均通过。方法声明提取的 3 项测试及
覆盖率工具的 2 项测试通过；客户端校验仍为 172 个方法对应 179 个组件声明。

## Test coverage

范围：全部 14 个 Rust workspace package，默认 features、默认文件过滤，无额外文件排除；
macOS arm64、rustc 1.98.1、cargo-llvm-cov 0.8.4。15 项原有原生 Provider 用例需要本地安装、
已有会话或认证，按默认配置忽略，逐项原因见 JSON。Linux/Windows 和 TypeScript 覆盖率未测量。

| 范围 | 行覆盖率 | 已覆盖 / 总行数 |
| --- | ---: | ---: |
| workspace | 94.1424% | 55834 / 59308 |
| api | 94.2794% | 2126 / 2255 |
| browser | 97.5443% | 715 / 733 |
| daemon | 95.8651% | 881 / 919 |
| domain | 100.0000% | 121 / 121 |
| file | 95.4782% | 1837 / 1924 |
| filesystem | 94.8806% | 11565 / 12189 |
| metadata | 94.0880% | 5411 / 5751 |
| model | 96.7705% | 1678 / 1734 |
| protocol | 100.0000% | 63 / 63 |
| provider | 93.1513% | 26808 / 28779 |
| relay | 91.8489% | 462 / 503 |
| schedule | 97.5980% | 772 / 791 |
| terminal | 96.4624% | 1527 / 1583 |
| voice | 95.1605% | 1868 / 1963 |

前次报告为 13-crate workspace，早于 file 提取及生产文件归属迁移，缺少相同 crate/文件
范围的直接可比基线，因此不计算百分比差值。

新单文件 I/O 为 41/41，观察为 69/71，通用 registry 为 97/99，创建回执适配为 21/21，
relay RPC 为 38/39，metadata 方法声明为 21/21。未覆盖行为包括 watcher 阻塞任务异常终止、
daemon 配置的部分校验与 I/O/超限转换，以及旧 Provider 原生和辅助通道。relay 私有执行函数
的 MethodNotFound 后备分支无法从公开分发入口进入。后续修改这些行为时补充可控故障用例，
在具备对应原生环境时运行 opt-in 测试，并检查 Linux CI。

[可审阅 JSON 证据](file-and-rpc-boundaries-pr-coverage-2026-10-08.json)包含源文件摘要和 LCOV
零命中源行诊断；权威行总数来自 LLVM JSON summary。HTML 已生成到
`target/llvm-cov/html/index.html`，不提交运行时 HTML、日志或原始 profile。

## 精确命令

构建、测试和覆盖率使用 `RUST_BACKTRACE=1`，并将 `SHERPA_ONNX_LIB_DIR` 指向已有的
`target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`；实际环境路径见 JSON。

```sh
cargo fmt --all --check
cargo build --workspace --offline --locked
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast -- --test-threads=1
cargo llvm-cov --workspace --html --offline --locked -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path target/crate-boundaries-pr-coverage-raw.json
cargo llvm-cov report --lcov --output-path target/crate-boundaries-pr-coverage.lcov
python3 scripts/rust_method_specs_test.py
python3 scripts/check-paseo-client-methods.py
python3 scripts/check-crate-coverage.test.py
python3 -m py_compile scripts/paseo-focused-coverage.py
npm run check:docs
git diff --check
```
