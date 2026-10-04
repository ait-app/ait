# 在线服务入口与逐主机同步：PR 验证

日期：2026-10-05。范围：[ADR-083](../../decisions/clients/adr-083-online-service-host-sync.md)
的二级登录入口、应用在线服务设置、逐主机 daemon 控制和租约隔离。

测量基于 `3a13d02bea2c63592f61727e382678f76566a40c` 的本报告所在提交源码。
[可审查覆盖率摘要](online-service-host-sync-coverage.json) 保存变更 Rust 文件 SHA-256
和 845 个 Cargo / Rust workspace 文件的源码指纹
`799e2c556b95f46c69d8c23f2687a430ffed1050246eb825d23092c2bbcddafd`，
标识实际测量的工作树，不依赖文档自身提交哈希。

## 测试结果

普通 Rust 全仓串行测试：**1,784 通过、0 失败、3 忽略**。
覆盖率插桩全仓测试独立获得相同结果，两次计数不相加。
忽略的是既有真实 Claude / Codex CLI 测试，其中两项需要认证并执行推理；未增加忽略项。

客户端相关 **146 项定向测试通过**：mobile 11 个文件 125 项、SDK 12 项、desktop 9 项。
覆盖连接方式顺序及二级导航、退出后的远程同步和停止、绑定 daemon 的撤销失败、
远程授权失败隔离、切换账户后保留原租约归属，以及关闭连接后控制票据不重放。
Rust 新增接口回归覆盖能力协商、单连接路由、状态读取、无效票据、不安全中心地址、启动及停止。

本次使用真实界面组件和模拟账户 / daemon 状态检查添加连接、应用在线服务设置，
以及主机的待连接、在线、停止和移除客户端连接状态；没有修改真实账户或 daemon 配置。

变更的 TypeScript、Markdown 和 JSON 文件通过 `oxfmt --check`，TypeScript 文件通过 `oxlint`。
其余检查命令如下，全部通过：

```sh
cargo test --workspace --locked --offline -- --test-threads=1
cargo build --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
npm run typecheck --workspace=@ait/protocol --workspace=@ait/client --workspace=@ait/mobile --workspace=@ait/desktop
npm run check:docs
python3 scripts/check-paseo-client-methods.py
git diff --check
```

全量并发运行中，原有
`websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints`
等待 WebSocket 消息超时。在相同源码下定向串行复查通过，最终普通全量及覆盖率全量串行运行也通过。
没有修改该测试或目录同步实现，未确定超时根因；再次发生时应独立调查。

## Test coverage

| 测量范围                    | 覆盖行 / 总行   | 行覆盖率 |
| --------------------------- | --------------- | -------- |
| Rust workspace              | 49,214 / 52,090 | 94.48%   |
| `api`                       | 1,975 / 2,134   | 92.55%   |
| `daemon`                    | 918 / 963       | 95.33%   |
| `relay`                     | 422 / 464       | 90.95%   |
| 新增 `api/src/relay_rpc.rs` | 28 / 30         | 93.33%   |

平台：macOS aarch64，Rust 1.98.1 / LLVM 22.1.8，cargo-llvm-cov 0.8.4。
使用默认 Cargo features 和工具默认源码过滤，没有额外排除生产文件；测试模块、doctest
及 build script 不计入行覆盖率。Linux、Windows、Android 和 iOS 未进行 Rust 覆盖率测量。
没有同平台、同工具版本的可比较基线，未报告覆盖率差值；历史 Linux 报告不作为本次基线。

使用全新的独立插桩构建目录，避免旧源码映射混入。以下命令继承
`SHERPA_ONNX_LIB_DIR`，指向本机预装的 sherpa-onnx v1.13.8 macOS ARM64 静态库，
无需下载原生库：

```sh
export CARGO_LLVM_COV_TARGET_DIR="$PWD/.tmp/online-service-pr/llvm-cov-target"
cargo llvm-cov --workspace --html --locked --offline -- --test-threads=1
cargo llvm-cov report --json --summary-only --locked --offline --output-path .tmp/online-service-pr/coverage-summary.json
cargo llvm-cov report --lcov --locked --offline --output-path .tmp/online-service-pr/coverage.lcov
```

共享的 [JSON 摘要](online-service-host-sync-coverage.json) 包含命令、源码指纹、各 crate 计数、
变更生产文件和 LCOV 未命中行。HTML 已生成并检查，位于 `target/llvm-cov/html/index.html`；
HTML 和原始运行日志属于本地构建产物，不提交仓库。

新增接口未覆盖的可执行行是传输 / 协议错误映射与防御性 MethodNotFound 分支。
真实线上中心 TLS 互通、发送队列关闭、全部异步取消竞争和 Android 真机端到端流程未完成本次验证，
后续需要故障注入及真实服务互通验收。以上百分比只衡量 Rust 可执行行，
不代表 TypeScript 界面或账户状态机的覆盖率。

## 行为限制

其他 daemon 的续租仍依赖运行中的平台账户管理器。退出账户不撤销这些租约，
关闭应用也不主动删除它们；进程停止或原账户授权失效后无法继续续租。
重启应用需要再次启用主机同步。daemon 不保存持久账户凭据，不能在应用退出后独立续租。
