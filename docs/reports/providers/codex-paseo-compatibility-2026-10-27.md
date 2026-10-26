# Codex / Paseo 兼容验证（2026-10-27）

## 范围与源码

本次检查和修复基于 Ait `365d536dd3544428c4b4348c5f74443bfc1cfc15` 的工作树，
限于 Codex provider、共用工具详情投影和直接相关的消息接纳、订阅与恢复回归。
实现仍由 Rust provider adapter 拥有，不新增客户端协议或持久化实体。

- Paseo 对照固定到
  [`4f9d9d6de2236486ecc2ad886a2c37f4b92271e0`](https://github.com/getpaseo/paseo/tree/4f9d9d6de2236486ecc2ad886a2c37f4b92271e0/packages/server/src/server/agent/providers)。
  最初获取的 `28834a91aeb31c4058bfa92558f9b45b5e5121c0` 与最终提交之间仅修改 ACP
  和 connected-hosts 测试，Codex 源码未变化。
- 对照文件包括 `codex-app-server-agent.ts`、`codex/app-server-transport.ts`、
  `codex/tool-call-mapper.ts`、`codex/rewind.ts`、`codex/options.ts` 和
  `codex/async-questions.ts`。旧本地 Paseo 0.12.0 未作为最新协议依据。
- 原生协议通过安装的 Codex CLI **0.164.0** 生成 JSON schema，并参考
  [官方 app-server 文档](https://learn.chatgpt.com/docs/app-server)。
- 执行平台为 macOS；Linux、Windows 原生进程行为未在本次环境执行。

边界与资源决策见 [ADR-193](../../decisions/providers/adr-193-codex-native-protocol-compatibility.md)。
第三方来源记录在 [Paseo NOTICE](../../../third-party/paseo/NOTICE)。

## 修复矩阵

| 问题                                               | 修复后的行为                                                          | 主要回归位置                                                                                |
| -------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| 每轮重建 sandbox 清除原生配置                      | 保存 thread 返回的有效策略；切换模式时合并继承设置和明确覆盖          | `local/codex/sandbox/tests.rs`、`tests/configuration.rs`                                    |
| 完成先于 interrupt 导致替换消息失败                | 仅把精确的原生 already-idle 拒绝视为幂等成功；完成事件继续结算        | `local/codex/tests.rs`、`service/agent_execution/tests/delivery.rs`                         |
| 新版 `subAgentActivity` 不可见                     | 新旧活动均投影工具详情并建立经验证的父子关系，保留子会话独立事件归属  | `local/codex/subagents/tests/activity.rs`、`service/agent_execution/tests/subscriptions.rs` |
| 等待 RPC 时超过 128 条通知中断进程                 | 内存保留最多 128 条，溢出写私有临时文件，按顺序回放                   | `local/codex/transport/tests.rs`、`transport/events/tests.rs`                               |
| 空 reasoning summary 隐藏正文                      | summary 无文本时回退到非空 content                                    | `local/codex/discovery/tests.rs`                                                            |
| 交互恢复归档 thread 失败                           | 精确归档拒绝后取消归档并重试一次；只读历史不修改归档状态              | `local/codex/tests/persistence.rs`、`native_sessions/tests.rs`                              |
| 未知斜杠文本、根路径被当作缺失 skill 拒绝          | 只有目录中存在的命令才展开，其余保留原文                              | `local/codex/tests.rs`、`controls/tests.rs`                                                 |
| terminalInteraction 被过滤，子会话 Timeline 被丢弃 | 投影 stdin 和有界进程标签，根与子会话都按原生归属发布                 | `local/codex/terminal/tests.rs`、`tests/progress.rs`、`subagents/tests/activity.rs`         |
| 旧 user-input 方法及非阻塞问题被拒绝               | 支持两种原生回调名称，非阻塞问题仍可回答                              | `local/codex/tests.rs`、`permissions/tests/paseo.rs`                                        |
| prompt 目录使用宿主 HOME，坏文件隐藏其他命令       | 按会话 CODEX_HOME / HOME 解析；缺失命令保留原文；跳过单个坏文件       | `local/codex/tests.rs`、`prompts/tests.rs`                                                  |
| 压缩缺少 final item 持续 loading，旧通知不释放准入 | 终态补齐压缩完成，新旧通知配对去重，释放手动压缩准入                  | `local/codex/streaming/tests.rs`、`tests/progress.rs`、`subagents/tests/activity.rs`        |
| 未指定模型时 runtime info 丢失原生默认模型         | 仅在明确指定新模型时覆盖已解析值                                      | `local/codex/tests.rs`                                                                      |
| 辅助摘要把原生重试或旧 turn 错误当作失败           | 仅当前 turn 的不可重试错误终止收集                                    | `local/codex/metadata/tests.rs`                                                             |
| systemError thread 无法读取已完成历史              | thread 错误状态不等于活跃 turn；仍检查 turn 是否进行中                | `local/codex/native_sessions/tests.rs`                                                      |
| 非首轮 rewind 使用已失效的 lastTurnId              | 统一排他 beforeTurnId 并传递线程配置；旧服务器仅在新 fork 上 rollback | `local/codex/rewind/tests.rs`、`tests/installed/offline.rs`                                 |
| 异步问题覆盖最终 assistant 回答                    | async 交互项不更新最终回答缓存                                        | `local/codex/tests.rs`                                                                      |
| 子进程退出时 EOF 失败未标记连接关闭                | EOF、异常响应和暂存回放失败都进入关闭状态，保留失败诊断               | `local/codex/transport/tests.rs`、`transport/events/tests.rs`                               |

其中异步问题回归在修复前得到 `Completed(None)`，修复后保留 `Final answer`。
RPC 突发测试覆盖 1024 条事件、读操作未完成时继续追加、文件读写失败和顺序释放。
错误文本使用有界 `SecretString`，并有 Debug 脱敏回归。

## 验证

### 初次修复验证

以下命令均使用当前工作树、Cargo.lock 和默认 features。`RUSTC_WRAPPER=` 避免本机
sccache 在受限执行环境中失败。涉及子进程检查和 loopback 监听的测试在允许相应系统
操作的执行环境中运行；没有通过降低并行度回避失败。

| 命令                                                                                                                                                              | 结果                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| `RUSTC_WRAPPER= cargo test --quiet --locked --offline -p provider --lib local::codex`                                                                             | 210 passed，0 failed，2 ignored          |
| `RUSTC_WRAPPER= cargo test --quiet --locked --offline -p provider --lib service::agent_execution::tests`                                                          | 101 passed，0 failed                     |
| `RUSTC_WRAPPER= cargo test --quiet --locked --offline -p provider --lib local::tool_detail`                                                                       | 8 passed，0 failed                       |
| `RUSTC_WRAPPER= cargo test --quiet --locked --offline -p provider --lib installed_codex_offline_turns_rewind_and_unarchive_use_the_current_protocol -- --ignored` | 1 passed，0 failed；显式启用离线原生集成 |
| `RUSTC_WRAPPER= cargo clippy --locked --offline -p provider --all-targets -- -D warnings`                                                                         | 通过，0 warnings                         |
| `RUSTC_WRAPPER= cargo check --locked --offline --workspace`                                                                                                       | 通过，根 workspace 可编译                |
| `cargo fmt --all --check`、`git diff --check`、`npm run check:docs`                                                                                               | 全部通过                                 |

安装版离线集成使用独立临时 CODEX_HOME 和本机 Responses SSE 夹具，真实执行两轮、
历史重载、非首轮排他 fork、原会话不变校验，以及 archive 后的交互恢复。断言共两次
loopback 模型请求，未访问付费模型或使用账号凭据。默认忽略的另一项安装版测试会访问
真实模型，本次未启用。原生 sandbox 额外探针也确认继承的网络权限、临时目录限制和额外
可写目录未被覆盖，模型请求为零。

文档使用 `node_modules/.bin/oxfmt --check` 校验，Python 协议夹具通过 AST 语法检查。

首次直接相关服务回归揭示两处旧测试假设，已更新：使用明确的原生 RPC 拒绝测试消息隔离，
并以规范化路径比较 macOS `/var` 与 `/private/var` 别名。没有修改其他 provider 的实现。

### 创建 PR 时与最新主线集成

PR 分支基于 `a8db46db3874002e34741ba1bd66426d1382f0f0`（0.1.19），保留主线新增的
实时语音、canonical history 分页、精确 writer-busy 判定和旧协议降级。
ADR 编号由已被主线占用的 189 调整为 193，引用同步更新。

语音关闭等待改用相同的有界内存 / 临时文件队列，保留文本、审批、canonical speech 与
关闭通知之后剩余事件的顺序。新增回归覆盖 512 条文本、canonical speech、丢弃旧媒体及
关闭后的文本尾部。原有 17 项修复范围不变。

新主线的 Opus 构建需要 CMake；验证使用临时目录安装的 CMake 4.4.4，通过环境变量
`CMAKE=/private/tmp/ait-codex-pr-build-tools/cmake/data/bin/cmake` 指定，不修改仓库依赖。

主线集成后的定向回归共 **361 passed，0 failed**，命令仍为上表的四组 provider 过滤测试：

| 范围                              | 结果                  |
| --------------------------------- | --------------------- |
| `local::codex`                    | 242 passed，4 ignored |
| `service::agent_execution::tests` | 110 passed            |
| `local::tool_detail`              | 8 passed              |
| 显式启用的安装版离线集成          | 1 passed              |

新增的两个原生语音测试也要求登录 / 上游网络，未启用；因此默认 Codex 过滤范围有四个 ignored，
其中离线集成已单独显式运行。其余需要账号或付费模型的测试仍跳过。

最终提交检查通过：

- `RUSTC_WRAPPER= cargo clippy --locked --offline --workspace --all-targets -- -D warnings`：0 warnings。
- `RUSTC_WRAPPER= cargo build --locked --offline --workspace`：根 workspace 可构建。
- `cargo fmt --all --check`、文档格式、Python 夹具 AST、`npm run check:docs` 和 `git diff --check`：通过。

上述 Rust 命令同时使用临时 CMake 环境变量。workspace 范围仅 lint / 构建，没有执行全量测试。

## 结论与限制

本次对照所确认的问题均已修复；当前固定版本对照未发现其他已确认的实现问题。
这是一份已执行范围内的验证记录，不代表未来 Codex 版本或所有外部工具组合都无缺陷。

既有文本、工具预览、timeline 和子会话数量预算继续生效。事件溢出暂存引入临时磁盘使用，
存储不可用或超时仍明确失败；临时文件随消费、关闭或丢弃自动释放，不进入仓库或原生历史。
认证、工具执行和原生历史仍由 Codex 拥有。未运行 Rust 全 workspace 测试，也未扩展到
无关 provider 的最终验收；workspace 检查仅编译。

## Test coverage

**not measured**：用户未请求覆盖率测量，测试通过数量不等于行覆盖率，历史报告的覆盖率
也不适用于本次修改。若后续请求测量，使用 `cargo llvm-cov` 限定 provider 的 Codex、
工具投影及直接相关服务路径，并提供精确版本、命令、行数和可审阅的报告产物。
