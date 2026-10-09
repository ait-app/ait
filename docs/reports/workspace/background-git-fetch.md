# 活跃工作区后台 Git fetch 实施报告

Ait 补齐了 Paseo 的远端引用刷新：活跃目录订阅首次观察仓库时立即执行
`git fetch origin --prune --quiet`，之后每 180 秒刷新。远端 main 已更新而本地 main
尚未移动时，“从 main 更新”能够识别并合并刚获取的提交。
决策与上游依据见 [ADR-069](../../decisions/workspace/adr-069-background-git-fetch.md)。

## 实现

- metadata 用消费者端口登记符合目录订阅过滤条件的、未归档工作区。初始响应入队后
  才激活观察；筛选变化、归档、项目删除、订阅释放及断线会移除相应路径。
  普通目录读取不启动后台刷新。
- filesystem 按规范化的 Git common directory 共享任务。多个客户端和 linked worktree
  不重复获取同一仓库；每仓库不重叠，整个进程最多同时执行两个 fetch。独立阻塞预算
  保留前台 Checkout 操作的可用性。
- 非 Git、裸仓库及没有 origin 的目录跳过，每 180 秒重新发现，支持后来添加 origin。
  fetch 禁止终端凭据提示、120 秒超时；最后一个观察者释放或服务关闭会取消并回收进程。
  错误只记录脱敏类别，下一轮重试。
- fetch 后发布去重的 `checkout.status.update`。能力协商只在目录和生产者均安装时
  声明 `checkout-git-events-v1`；App adapter 转成已有 SDK 事件并补齐空 `requestId`，
  由现有缓存处理刷新状态、比较和提交列表。
- 状态及 base diff 对未限定分支优先比较 origin 引用；完全限定引用保持原义。
  提交列表与 merge-from-base 使用本地和 origin 中进展更大的基准，避免远端提交被
  误算成当前工作区变更。按钮本身仍只合并已有引用，不额外等待网络 fetch。

生产实现：[观察协调](../../../crates/filesystem/src/git/service/git_fetch.rs)、
[Git adapter](../../../crates/filesystem/src/git/local/git_fetch.rs)、
[目录订阅](../../../crates/metadata/src/rpc/directory/listing.rs)、
[App 事件映射](../../../apps/mobile/src/runtime/rust-daemon/messages.ts)。

## 测试执行

验证来源：基线 `1cbe77b5f3f2056975d31f78769b26738183d4e3` 加本 PR 改动；
2026-10-02，Rust 1.98.1，macOS `aarch64-apple-darwin`，默认 features，锁定依赖并离线运行。
Rust 构建目录为 `/tmp/ait-git-fetch-target`。

| 检查                                                                       | 结果                                                 |
| -------------------------------------------------------------------------- | ---------------------------------------------------- |
| `cargo test --workspace --locked --offline`                                | 1,530 passed、0 failed、3 ignored                    |
| `cargo build --workspace --locked --offline`                               | 通过，无编译警告                                     |
| `cargo llvm-cov --workspace --html --locked --offline`                     | 1,530 passed、0 failed、3 ignored，HTML 已生成并审阅 |
| App 事件映射与 checkout cache 单测                                         | 28 passed、0 failed                                  |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 通过                                                 |
| `cargo fmt --all --check`、`git diff --check`                              | 通过                                                 |
| App TypeScript `tsgo --noEmit`                                             | 通过                                                 |
| 两个修改的 TS 文件 `oxfmt --check`、`oxlint --deny-warnings`               | 通过                                                 |

完整测试包含新的真实本地 remote 和 WebSocket 流程，以及最新 main 的工作区运行时摘要
回归。3 项既有测试因需要真实 Claude/Codex CLI 或登录授权，保留默认 ignored 状态。
测试数量独立于下方覆盖率结果。

```sh
export CARGO_TARGET_DIR=/tmp/ait-git-fetch-target
export SHERPA_ONNX_LIB_DIR=/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib
cargo test --workspace --locked --offline
cargo build --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
npm run test --workspace=@ait/mobile -- --project unit \
  src/runtime/rust-daemon/messages.test.ts src/git/checkout-status-cache.test.ts
node_modules/.bin/tsgo --noEmit -p apps/mobile/tsconfig.json
node_modules/.bin/oxfmt --check apps/mobile/src/runtime/rust-daemon/messages.ts \
  apps/mobile/src/runtime/rust-daemon/messages.test.ts
node_modules/.bin/oxlint --deny-warnings apps/mobile/src/runtime/rust-daemon/messages.ts \
  apps/mobile/src/runtime/rust-daemon/messages.test.ts
git diff --check
```

API/进程测试需要本机 loopback socket，获准后在沙箱外运行。默认语音依赖使用已有的
Sherpa ONNX 静态库缓存；前端复用本机已安装依赖，未做干净的依赖安装。

可审阅测试：[定时、共享任务与取消](../../../crates/filesystem/src/git/service/git_fetch/tests.rs)、
[真实 fetch/prune 和进程回收](../../../crates/filesystem/src/git/local/git_fetch/tests.rs)、
[目录筛选和归档](../../../crates/metadata/src/service/directory/tests/git_observation.rs)、
[基准引用](../../../crates/filesystem/src/git/local/checkout/tests/remote_base.rs)、
[真实 server WebSocket 流程](../../../bins/daemon/tests/process/git_fetch.rs)、
[SDK schema 验证](../../../apps/mobile/src/runtime/rust-daemon/messages.test.ts)。

## Test coverage

测量版本为上述基线加本 PR 的 Rust 源码，聚合 SHA-256 和逐文件 SHA-256 记录于
[共享覆盖率 JSON](background-git-fetch-coverage.json)。测量后仅修改文档及该 artifact。
范围为完整 Cargo workspace、默认 features、macOS `aarch64-apple-darwin`，Rust 1.98.1、
cargo-llvm-cov 0.8.4。使用默认源文件过滤，无自定义排除；默认不插桩 doctest，
不测量 TypeScript/UI 覆盖率。3 项既有真实 CLI/授权测试保持忽略。

```sh
export CARGO_TARGET_DIR=/tmp/ait-git-fetch-target
export SHERPA_ONNX_LIB_DIR=/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib
cargo llvm-cov --workspace --html --locked --offline
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-background-git-fetch-coverage-summary.json
cargo llvm-cov report --lcov --output-path /tmp/ait-background-git-fetch-coverage.lcov
```

| 范围                          | 行覆盖率 | 已覆盖 / 总行数 |
| ----------------------------- | -------: | --------------: |
| Rust workspace                |   92.10% | 43,610 / 47,352 |
| filesystem                    |   89.86% | 10,169 / 11,317 |
| metadata                      |   90.78% |   7,022 / 7,735 |
| api                           |   95.56% |   1,658 / 1,735 |
| daemon                        |   94.38% |       890 / 943 |
| 新增 fetch adapter 与协调服务 |   97.18% |       345 / 355 |

相对 main 的[最近同口径报告](workspace-sidebar-runtime.md) 92.0119%
（43,160 / 46,907）增加 0.0856 个百分点；没有重新测量精确基线，故此为历史比较，
不把全部差值归因于本 PR。HTML 位于本地
`/tmp/ait-git-fetch-target/llvm-cov/html/index.html`；已提交的 JSON 是共享审阅 artifact，
包含各 crate、修改的生产文件、LCOV 新增行统计及未覆盖行号。

新增代码未覆盖的行包括子进程 `try_wait` 的系统错误、阻塞任务异常返回、部分取消竞态、
空观察路径及备用组合分支。下一步可用故障注入补充这些分支，并在 Linux/Windows 上
验证进程树回收。测试通过数量记录在上方，不能由行覆盖率推导所有平台均已验收。

## 验证边界

真实 Git 验证使用临时本地 bare remote；没有连接真实托管平台，没有推送用户仓库。
本轮未运行 Linux/Windows 平台测试，未操作已安装的 Desktop 界面或发布安装包。
网络失败会保留上一次远端引用；180 秒后台刷新不保证点击瞬间的远端状态。
