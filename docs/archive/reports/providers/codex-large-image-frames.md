# Codex 大图片事件导致 Rust 会话失败

日期：2026-09-28。基线：`ca317d2b1c3eb0c2e02112a4eae73425ea7248fa`，验证对象为其上的本地修改。

## 故障与复现

原生会话连续产生两条图片完成事件：JSONL 记录分别约 1.98 MB 和 2.14 MB。
第一张进入 Rust timeline，第二张没有进入；第二条原生事件的时间与 Rust Agent
进入 `Provider execution failed` 的时间吻合。旧 Codex transport 使用
`take(2 * 1024 * 1024).read_until(...)`，超过上限后读不到换行就结束 reader，
随后 manager 关闭原生进程并保存失败状态。server 主进程没有崩溃。

离线回归用 3 MiB 图片 payload 复现了同样的错误：消息先被接受，等待结果却返回
`error`。在请求响应前收到大图片时，另一个 transport 回归也复现了连接失败。
截图中的 `Timeout waiting for message (60000ms)` 来自客户端；未保留当时的
WebSocket 报文，因此不声称重建了该 UI 超时的完整时序。

## Paseo 对照与修复

对照版本为仓库固定的 `getpaseo/paseo@2c8e8a826810337492cc5a38bb0bbd705b6fb632`。
已读取并核对该提交的本地源码。

| 路径                 | Paseo 原版                                        | 本次 Rust 行为                                                    |
| -------------------- | ------------------------------------------------- | ----------------------------------------------------------------- |
| Codex stdout JSONL   | `readline.createInterface`，无应用层单行大小限制  | 移除接收端 2 MiB 截断，完整读取一行                               |
| 原生图片 base64 落盘 | 解码并按内容 hash 写入私有目录，无 2 MiB 大小检查 | 移除原生图片输出的 2 MiB 检查，保留格式、目录权限和内容一致性校验 |
| 图片有 `savedPath`   | 优先使用现成路径                                  | 保留路径优先，投影只构造必要字段，避免复制整份 base64             |
| 通用 JSONL RPC v2    | 单帧 1 MiB、重组 64 MiB                           | 不套用到 Codex；原版 Codex transport 没有使用此 decoder           |

源码依据：

- [Codex app-server transport](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/providers/codex/app-server-transport.ts)
- [Provider image output](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/providers/provider-image-output.ts)
- [Codex image mapping](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/providers/codex-app-server-agent.ts)
- [通用 JSONL frame decoder](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/providers/jsonl-frame-decoder.ts)

本次只对齐原生输出大小策略。客户端请求、WebSocket、发送队列和 Codex 请求超时
保持既有语义；出站 2 MiB 常量改名为 `MAX_OUTBOUND_FRAME` 以明确作用范围。
与 Paseo 一样，完整原生行会在内存中解析，内存使用随单条原生输出大小增长。
没有新增 crate 边界或 RPC。旧报告中的“Codex 接收帧 2 MiB”描述由本报告取代。

## 验证

- 修复前两项新增复现测试均失败；修复后大图、历史刷新和后续消息通过。
- `cargo test --locked --offline -p provider local::codex:: -- --nocapture`：
  121 项通过，1 项受沙箱禁止 `ps` 影响，1 项真实模型测试保持 ignored。
- 受限项单独在沙箱外重跑：
  `cargo test --locked --offline -p provider local::codex::tests::closing_or_dropping_a_session_terminates_its_tool_process_group -- --exact`：1 项通过。
- `cargo test --locked --offline -p provider service::agent_execution::tests::streaming:: -- --nocapture`：4 项通过。
- `cargo test --locked --offline -p provider local::images::tests::`：6 项通过，覆盖图片落盘、权限、校验和大图片输出。
- `cargo clippy --locked --offline -p provider -p daemon --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`、`git diff --check`、Python fixture AST 解析：通过。
- `cargo build --locked --offline -p daemon --bin daemon`：通过。

共 132 项定向测试通过，1 项真实模型测试 ignored。已保留原 token 与数据重启
`127.0.0.1:4567`；`/healthz`、`/readyz` 和鉴权后的 `/v1/server/info` 均为 HTTP 200。
随后通过真实 WebSocket 执行 `agent.refresh.request`，113 ms 返回
`agent_refreshed`、状态 `idle`，原生历史投影恢复 33 条完整条目。
读取持久化结果确认 `lastError` 清空，两张生成图片都已恢复。
刷新只读取原生历史并更新本地投影，没有发送新提示或调用模型。

## Test coverage

提交准备时已完成完整测试和覆盖率测量。普通测试及 instrumented 测试均为
**1270 passed、0 failed、3 ignored**。三个 ignored 用例需要本机 Claude CLI 或
Codex/Claude 真实认证及模型调用，保持仓库默认配置；这不是覆盖率百分比。

| 测量范围                   | 已覆盖 / 总行数 | 行覆盖率 |
| -------------------------- | --------------: | -------: |
| Rust workspace             | 35,722 / 38,995 | 91.6066% |
| provider                   | 15,244 / 16,409 | 92.9002% |
| Codex transport            |       230 / 250 | 92.0000% |
| Codex discovery / 图片投影 |       232 / 245 | 94.6939% |
| 原生图片存储               |       144 / 156 | 92.3077% |

测量版本是 `ca317d2b1c3eb0c2e02112a4eae73425ea7248fa` 加本次源码修改；
[可审阅的覆盖率产物](codex-large-image-frames-coverage.json)记录每个修改文件的 SHA-256、
workspace / 各 crate 统计和验证结果。HTML 已生成于 `target/llvm-cov/html/index.html`；
随 PR 提交 JSON 产物以供其他人审阅。

相同范围的[已有基线](../workspace/workspace-create-worktree-coverage.json)记录的 246 个源码 hash
均与本次基线提交相同。workspace 从 91.6026% 增至 91.6066%（+0.0041 个百分点），
provider 从 92.8902% 增至 92.9002%（+0.0100 个百分点）。

精确命令：

```sh
cargo test --workspace --locked --offline
cargo build --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
cargo llvm-cov --workspace --html --locked --offline -- --test-threads=4
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-large-image-pr-coverage.json
```

测量为 macOS aarch64、workspace 默认 features，没有额外排除文件或跳过测试；
测试源码排除和 doctest instrumentation 使用 cargo-llvm-cov 默认值。
Linux / Windows 不在本次本地测量范围内。剩余缺口包括真实 Provider 集成测试和
既有进程清理、原生协议异常、图片文件系统失败分支，后续由平台 CI 和定向异常测试补齐。
新增回归覆盖大图完成通知、忽略 legacy 镜像、响应前事件顺序、8 MiB 历史响应、
无路径的大图落盘、失败状态不再出现、历史刷新和继续发送。
