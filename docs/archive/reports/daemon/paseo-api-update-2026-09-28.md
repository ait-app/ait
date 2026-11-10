# Paseo 0.10.0-beta.1 API 对照与搜索移植

- 日期：2026-09-28。
- 初次移植基线：`ccc33a58d0721c0eaaa37948cdfeedfdaffc52b9`。
- PR 验证基线：`b04726a1083fa4f0a6ee516242560c517eea77df` 加本次修改；已同步最新 main。
- 原导入来源：Paseo `0.9.0-beta.2`，`2c8e8a826810337492cc5a38bb0bbd705b6fb632`。
- 本地更新来源：`/Users/necokeine/Documents/paseo`，`0.10.0-beta.1`，
  `30178c4f58b67f8472901356e1484022bd835de0`；对照时工作区干净。
- 范围：共享 protocol、client SDK、服务端对应行为及调用这些 API 的聊天搜索界面。

## API 差异和取舍

独立提取新版本 `SessionInboundMessageSchema`，与现有固定来源 fixture 比较：仍是
**205 个入站名称，新增 0、删除 0**。不能把版本升级等同于新增业务 RPC；这次主要变化是
已有响应字段、WebSocket 握手消息和 SDK 辅助接口。历史导入清单及固定 fixture 保留原来源，
没有把整个项目标记成已经同步到新版 Paseo。

| 上游变化                                                                                              | 本次处理 | 原因                                                                                     |
| ----------------------------------------------------------------------------------------------------- | -------- | ---------------------------------------------------------------------------------------- |
| `agent.timeline.search.response.locations[].count`                                                    | 移植     | 整段聊天的匹配总数、全局位置和跨消息跳转直接改善现有搜索                                 |
| `hello.auth`：`password` / `localCredential`                                                          | 暂不移植 | Rust 服务已有 Bearer 与浏览器一次性票据，Paseo 的本地凭据和 relay admission 不能直接套入 |
| `hello.rejected`、`hello_rejection` capability                                                        | 暂不移植 | 属于上述认证握手；Rust transport 使用自己的协商和错误消息                                |
| `server_info.protocolVersion`                                                                         | 暂不移植 | Rust 已使用 `protocol.major/minor`；照搬 Paseo 的整数版本会混淆两套协议                  |
| SDK `localCredential`、`DaemonAuthenticationError`、`getDaemonAuthFailureReason`、`authFailureReason` | 暂不移植 | 应与密码握手完整接入，避免仅有类型而没有服务端行为                                       |
| `parseRelayConnectionUri` / `serializeRelayConnectionUri`、`createRelayTransportFactory`              | 暂不移植 | 面向 Paseo relay 配对及加密 hello；当前 Rust 服务没有该 relay 配对入口                   |
| Node daemon `getServerId()`、`isBearerTokenValidAsync`                                                | 暂不移植 | 仅影响上游 Node 运行时和其认证实现，当前运行时固定为 Rust                                |
| OpenCode v2、Pi 扩展适配                                                                              | 暂不移植 | 属于新增 Provider 内部集成，不是现有 Codex/Claude Rust 服务的业务 RPC 增量               |

认证相关变化来源为 `e1c769c01`（#5393）。搜索来源为
[`b2ce2bcb8`（#5167）](https://github.com/getpaseo/paseo/commit/b2ce2bcb83dd40bd91c32098c59221f0b52d7ffd)。
本次沿用 provider 的 Timeline 查询边界，没有改变领域归属或恢复被移除的能力组。
PR 已同步 ADR-061/062 的 Ait 清理，relay / plugin 仍遵循 main 的移除边界。

## 已移植行为

Rust `agent.timeline.search.request` 返回每条命中的正整数 `count`。每次请求只编译一次转义后的
字面查询，忽略大小写并容许单词间的空白变化，统计不重叠的出现次数。用户消息按字面文本搜索；
助手消息通过 `pulldown-cmark` 提取 Markdown 块的文本，支持粗体、行内代码、实体、代码块、
列表和表格，不把跨段落/列表项/单元格拼接当作命中，也不搜索链接目标或图片 alt 文本。
保留 4096 字节的规范化查询上限、200 条分页、原有消息分片合并、序列游标和响应大小限制。

Protocol 的 `count` 保持可选以兼容旧服务端；Zod 和生成的 AOT validator 都保留并验证该字段。
Rust transport 的响应封装和 SDK 类型已验证，不需要增加新方法映射。

前端搜索读取所有结果页后显示整段聊天的“第几处 / 共几处”，下一处/上一处跨消息移动并在首尾
循环。旧服务端未返回计数时，每条位置先计一次；消息显示后用实际渲染命中数校正估计，去除不可见
候选和合并后重复的位置。清理九种语言中只用于“消息内计数”的旧文案，复用已有全局位置翻译。

计数与上游一样是渲染前估计。Rust Markdown parser 与前端渲染器并非同一个实现，复杂 HTML、
渲染扩展等仍可能产生差异，由现有 reveal 校验修正。没有改变 Timeline 的存储/投影模型。

## 验证

PR 在同步 main 后重新验证，平台为 macOS arm64，Rust 默认 features。完整 workspace 测试由
`cargo llvm-cov --workspace --html` 执行：**1,279 通过、0 失败、3 ignored**。三个既有忽略项
需要本机 Claude/Codex CLI 或认证，未调用真实付费 Provider，没有新增忽略项。

前端定向测试共 **63 通过、0 失败、0 ignored**：Chat Find、Rust 响应适配与翻译一致性 51 项，
搜索响应 Zod / AOT 兼容及非法计数 12 项。SDK 构建、App 类型检查、workspace 构建、严格 Clippy、
Rust 格式、改动 TS 格式/lint 和 diff 检查均通过。未执行桌面 UI、Linux/Windows 或前端全量测试。

首次本地定向进程测试曾被沙箱禁止监听端口拦住；提交前完整测试在允许本机端口、PTY 和测试子进程的
环境运行，全部通过。所有 Provider 执行使用离线测试夹具。

```sh
cargo llvm-cov --workspace --html --locked --offline -- --test-threads=4
cargo build --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo fmt --all --check
npm run build:sdk
node_modules/.bin/tsgo --noEmit -p apps/mobile/tsconfig.json
npm run test --workspace=@getpaseo/protocol -- src/messages.timeline-search.test.ts
npm run test --workspace=@getpaseo/app -- --project unit src/agent-stream/chat-find/model.test.ts src/runtime/rust-daemon/messages.test.ts src/i18n/resources.test.ts
# 对本次修改的 TS/TSX 文件运行 oxfmt --check 和 oxlint；文档运行 oxfmt --check。
git diff HEAD --check
```

## Test coverage

测量版本为上述 PR 验证基线加[覆盖率制品](paseo-api-update-coverage.json)记录的源码 SHA-256。
范围为整个 Cargo workspace、默认 features、debug profile，macOS `aarch64-apple-darwin`，
Rust 1.98.1 / cargo-llvm-cov 0.8.4。没有显式文件排除，测试源码按工具默认过滤；未启用 doctest
coverage，当前 workspace 无 doctest 用例。未在 Linux/Windows 测量。

| 范围                      |   覆盖 / 总行数 | 行覆盖率 |
| ------------------------- | --------------: | -------: |
| Workspace                 | 35,787 / 39,062 |   91.62% |
| provider                  | 15,290 / 16,455 |   92.92% |
| `timeline.rs`             |       158 / 158 |  100.00% |
| `timeline/text_search.rs` |         55 / 55 |  100.00% |

main 上已有报告测量的是后续合并前的版本，没有此 PR 精确基线的同版本测量，因此不报告覆盖率增量。
上述行覆盖率与测试通过数量分别统计。改动的两个搜索生产模块没有未覆盖的可执行行；行覆盖率不等于
分支或渲染行为穷尽验证，复杂 Markdown/HTML 与前端渲染差异仍由 reveal 校正。真实 UI 和其他平台
留待对应环境验收。

```sh
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-paseo-api-pr-coverage.json
cargo llvm-cov report --show-missing-lines
```

HTML 位于 `target/llvm-cov/html/index.html`；随 PR 提交的 JSON 制品包含总计、逐 crate 和改动文件
的原始覆盖行数、测量命令、源码指纹及忽略原因，供远程审阅。原始 profile、运行日志和本地 HTML 不提交。
