# 大 Diff 加载与超限降级

日期：2026-10-03。PR 基线：main `6cfd8a1f5d0201b626b167df572ace8e9f7c6073`。
初始复现使用改名前的 main `3da672bbb7e0a74585f28c283d200172bafb0495` 和发布版 0.0.14。
修复位于 `codex/large-diff-loading`；本报告包含本地复现及 PR 提交前验证。

## 复现与原因

用户指定的 `eatable-kangaroo` 工作树在检查时没有未提交修改，分支 HEAD 为
`bee95a6d`。与 origin/main 的 merge-base 比较包含 3,844 个文件项、51,501 行增删，
Git 原始 Diff 为 4,035,182 字节，尚未达到 4 MiB 的 Git 输出限制。
使用已发布的 macOS arm64 0.0.14 Server，在隔离临时数据目录、随机本地端口和独立
WebSocket 连接中只读该工作树，约 34.9 秒后连接以 code 1001 关闭。

独立短行夹具也能复现：150,000 行 `a` 的 Git Diff 只有 450,122 字节，发布版
约 138 ms 后关闭 WebSocket。对照的 1,000 行 Diff 正常响应；1,500,000 行 Diff
原始输出为 4,500,123 字节，已有 `diffTooLarge` 提示能够正常返回，连接仍可继续查询状态。

RPC 原来仅在 JSON 超过预算时清除可选高亮 token，没有重新检查纯文本 JSON。
行对象、文件元数据和 JSON 转义仍可能使有效的 Git 输出超过 4 MiB 传输队列；
Outbound 无法入队后取消整条连接。新增四项回归测试在修复前全部失败。

修复后的优化构建在同一工作树约 32.1 秒返回 `diffTooLarge`（175 字节），
随后同一 WebSocket 上的状态查询成功。15 万短行用例约 105 ms 返回单文件
`too_large`（243 字节），同连接状态查询也成功。
原始复现结果见[定向验证摘要](large-diff-loading-validation.json)。

## Paseo 对照与修复

对照本地 Paseo `bd3986d964b3ff528a8adf045a062dde10616934` 和
[上游 Checkout 源码](https://github.com/getpaseo/paseo/blob/bd3986d964b3ff528a8adf045a062dde10616934/packages/server/src/utils/checkout-git.ts)：
单文件原始 Diff 限制为 1 MiB，累计原始文本为 2 MiB；结构化 JSON 预算按照
32 MiB 中继帧的加密和 Base64 扩张计算，再预留 1 MiB，结果约为 23 MiB。
单文件超限保留文件名和统计并返回 `too_large`；结构化快照超限返回 `diffTooLarge`。

Ait 沿用当前 4 MiB Git 输出和传输队列限制，采用已有的占位与快照提示行为：

- 先移除可选高亮，文本仍能满足预算时继续显示。
- 纯文本单文件仍超限时清空 hunks，保留文件名和统计，返回 `too_large`；其他文件继续显示。
- 完整快照仍超限时返回空文件内容和 `diffTooLarge`，避免超限消息进入连接队列。
- 完整快照按实际 JSON 编码检查，包括 cwd；仍预留 64 KiB 给协议封装和在途消息。
- 请求、订阅初始快照、订阅更新及提交文件 Diff 复用既有投影；缩小 Diff 后恢复内容。

没有修改 Outbound 的全局背压策略或增加传输预算，没有新增协议字段或依赖。
原有高亮 ADR 的预算说明已补充：[ADR-071](../../../decisions/workspace/adr-071-checkout-diff-syntax-highlighting.md)。

## 验证

定向验证先确认行为修复，提交准备阶段再执行完整 workspace 检查。
命令按 ADR-072 使用当前包名；重命名前的定向用例也包含在最终全量测试中。

- `cargo test --locked --offline -p filesystem rpc::checkout::tests`：14 passed。
  覆盖高亮保留/降级、纯文本超限、JSON 转义膨胀、累计快照预算、订阅去重与恢复。
- `cargo test --locked --offline -p daemon --test process large_diff`：2 passed。
  使用真实 Git、daemon 进程和 WebSocket，覆盖大文件初始响应、提交历史、订阅初始超限、
  大小变化后的事件及同连接后续状态查询。
- `cargo clippy --locked --offline -p filesystem -p daemon --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`、`git diff --check`：通过。
- Mobile `npm run typecheck --workspace=@ait/mobile`：通过。
- `E2E_AIT_SERVER_BIN=/private/tmp/ait-git-fetch-target/debug/daemon npm exec --workspace=@ait/mobile -- playwright test --project=browser e2e/browser/diff-size-limit.spec.ts`：2 passed。
  验证大文件提示与小文件内容实际画布绘制、Changes 视图整体超限提示、Git 原始输出超限提示、
  两次缩小后的恢复，并监听浏览器 WebSocket 确认过程中 Host 没有重连。
  补充实际连接数量断言后，另有一次 `--grep 'oversized snapshots'` 定向复验通过；
  与 main 合并后再次完整运行这两个用例。
- 新增 E2E 文件的 Oxfmt / Oxlint 检查通过；重命名前的修复版优化构建用于上述真实工作树验证。

Rust 定向命令使用 macOS arm64、默认 features，设置
`CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target` 及
`SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`。
运行中的用户 Host 没有重启，用户工作树未写入。

## 提交前验证

- `cargo test --locked --offline --workspace`：1,564 passed、0 failed、3 ignored。
- `cargo build --locked --offline --workspace`：通过，无编译警告。
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`、Mobile TypeScript、修改文件 Oxfmt / Oxlint、文档链接检查、`git diff --check`：通过。
- 全量 coverage 测试：1,564 passed、0 failed、3 ignored。
  3 项 ignored 为已安装 Claude 的模型发现、真实 Claude 原生回合和真实 Codex 原生回合，
  由现有测试声明决定；本次没有新增跳过。

## Test coverage

使用 cargo-llvm-cov 0.8.4 测量完整 Cargo workspace，macOS arm64、Rust 1.98.1、
默认 features、默认文件过滤，没有额外排除；不包含 TypeScript 或 doctest 插桩。
测试数量与行覆盖率分别报告。

| 范围              | 行覆盖率   | covered / total     |
| ----------------- | ---------- | ------------------- |
| Workspace         | **92.16%** | **43,958 / 47,697** |
| filesystem        | **90.23%** | **10,560 / 11,703** |
| Checkout RPC 模块 | **86.08%** | **470 / 546**       |

测量对象为基线 main 加本 PR 的 Rust 改动；精确源码指纹、修改的 Rust 源码 SHA-256、
workspace / crate / 修改模块的行计数和测试结果见[可审阅覆盖率证据](large-diff-loading-coverage.json)。
整体 Rust 源码指纹为
`d853bdd3003e4241779f776b91d5972cc0761561d4aed91ec90dbadd8fdd9e33`。
没有在同一环境重新测量目标 main，不计算与历史报告的覆盖率差值。

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --locked --offline --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-large-diff/workspace-coverage-summary.json
```

HTML 报告已生成于 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`；
共享证据为本 PR 中的 JSON 摘要，HTML 和原始运行产物未纳入提交。
浏览器测试覆盖 Web 的实际提示绘制与恢复；未运行原生 iOS/Android、Electron 外壳或
Linux/Windows 验收。真实已认证 Provider 回合仍需具备相应环境后运行现有 ignored 测试。
大型分支的首次语法高亮耗时未在本次优化，修复目标是超限提示和连接保持可用。
