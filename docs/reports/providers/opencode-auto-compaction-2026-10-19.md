# OpenCode 长会话、用量与压缩排查

- 日期：2026-10-19。
- 基线：`976a555e41bd86603901dc69e19a5df1b23a0115`，结果对应本次工作区修改。
- 决策：[ADR-115](../../decisions/providers/adr-115-opencode-acp-provider.md)、
  [ADR-121](../../decisions/providers/adr-121-acp-turn-history-reconciliation.md)。
  ACP 优先的原生观测边界见 [ADR-159](../../decisions/providers/adr-159-opencode-acp-native-observation.md)。

## gpu-desktop 的原生证据

2026-10-19 通过 SSH 只读检查截图对应的 Cirrus OpenCode 会话、Ait 持久状态和原生日志。
主机使用 OpenCode 1.18.4，模型为 `z-ai/glm-5.3`，窗口配置为 1,000,000 token。
用户配置的输出上限为 131,072；检查到的用户配置与 ACP 启动覆盖没有显式 `compaction` 设置。
排查没有发送模型输入、修改原生配置或数据库，也没有部署或重启远端服务。

- 最近一次有效 assistant 请求报告 input 251、cache.read 629,632，合计 629,883，约 63%。
  Ait 保存的 `lastUsage` 却为 0 / 1,000,000；原生最后一条被中断的 assistant 占位消息计数全部为零。
- 该会话没有 `compaction` part。没有压缩记录与配置的大窗口相符，不能认定压缩机制被 Ait 禁用。
- 最后两轮在停止之前仍有原生模型请求；部分完整步骤耗时约 7–12 分钟。
  末尾存在 `cancel` / `MessageAbortedError`，用户确认这是停顿之后手动停止或重启的操作，
  不能用这些记录解释之前的自行停顿。
- 另一次请求以原生 `finish=unknown` 结束，随后出现 `exiting loop`。
  1.18.4 ACP 对没有 `error` 的返回统一发出 `end_turn`，没有暴露这个区别。
  原生日志还记录过模型服务 `Bad Gateway`；目前没有足够证据把所有停顿归为同一个原因。

以上为读取到的状态与时间记录。下面分别修复可确认的计数、停止原因和压缩观测问题；
原生服务长时间没有响应的其他情况不能被本次回归视为已经消除。

## 修复误导的用量与终态

1. 原生 `usage_update.used=0` 不能证明会话上下文为空。
   已验证的 1.18 profile 从同一 ACP 子进程内部的官方只读接口补充每步原生用量，
   保留最近有效请求的 input + cache.read、原生窗口上限及累计 session 费用。
   不让中断占位消息覆盖有效计数，保留压缩后下降的正数，不累计不同模型步骤的输入。
   仅有零值而没有其他原生事实时继续显示“暂无数据”。
2. `max_tokens`、`max_turn_requests` 和 `refusal` 不再当作正常完成。
   原生历史结算后，通过已有持久错误提示说明停止原因；会话保留后续用户输入能力。
   `end_turn` 仍为正常完成，用户取消仍为取消，不自动提交 Continue。
3. 该 profile 同时读取本轮新 assistant 的原生 finish。
   明确的 `unknown` 不再被 ACP 的 `end_turn` 掩盖，显示固定停止原因后保留用户继续能力；
   上一轮的 unknown 不污染新输入，长模型请求不设新的宿主运行时限。

## ACP 优先的实时观测与恢复

消息、配置、工具、审批、取消、关闭和完整原生历史继续使用 ACP。
补充连接只对已验证的 OpenCode 1.18 profile 开启，只读取所属 ACP 子进程的 loopback server。
其他版本，包括 2.x，继续使用其原生 ACP 用量和 marker，不推测另一套 HTTP 协议。

本地 server 使用独立的临时 Basic Auth 凭据、显式 loopback 绑定和禁用 mDNS；
不附着用户已有进程，不启动第二个原生服务，不持久化密码、读取认证文件或访问原生数据库。
读取范围限制在原生模型上限、session 元数据、有界消息元数据页和 SSE；输入始终只发送一次 ACP prompt。

实时 compaction part 与 summary assistant 的 parentID 提供原生身份，转为已有 marker。
恢复时按 native 元数据补齐终态，并与 ACP 文本／工具历史合并；压缩专用空用户消息也保存
接纳轮次映射。快照不补造过去的开始事件，未闭合的压缩在 ACP 确认结束后显示失败。
订阅先于快照，断线后重新订阅并按事务刷新，循环游标和过大响应受限，失败不取消 ACP 请求。

## 自动压缩结算回归

OpenCode 自己执行自动压缩，Ait 的 ACP launcher 没有禁用它。
但此前结算要求一次 prompt 最多新增一条原生用户消息。
OpenCode 1.18.4 在自动压缩后创建 synthetic 继续消息，同一输入实际产生两条用户消息，
导致 Ait 在原生处理完成后的历史结算返回 `Failed`。

真实回归使用 `/usr/local/bin/opencode`（1.18.4）、独立 XDG 和原生数据库、loopback 模型。
模型上下文设为 32000、输出上限 2000；首次正常回答报告 31000 个输入 token，
触发原生自动压缩。修改前可重复失败，重读原生历史能看到原始输入与压缩后的继续消息。
模型服务只返回固定文本，不使用真实凭据或外部模型。

现在将本次原生 prompt 新增的所有用户消息关联到同一个已接纳输入，
保存到已有句柄映射，使历史回放和恢复后继续保留原轮次身份。
关联总数仍限制为 4096；超过预算不会保存不可恢复的句柄。
不修改原生压缩阈值、用户配置或原生数据库，不自动重发 prompt。

## 原生协议差异

OpenCode 1.18.4 的 ACP 在整轮 prompt 结束时发送 `usage_update`，
该版本的 ACP 也没有转发原生压缩生命周期。本次通过上述受管进程只读观测补齐这两个缺口。
因此仅凭截图中的 `0 / 1m` 不能证明压缩没有执行；本次另行读取原生历史确认确实无压缩。
账号用量的 `Unavailable` 与当前会话上下文计数是独立的能力。

原生行为的源码依据：
[1.18.4 ACP service](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/acp/service.ts)、
[1.18.4 ACP usage](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/acp/usage.ts)、
[1.18.4 ACP event](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/acp/event.ts)、
[1.18.4 压缩阈值](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/session/overflow.ts)、
[1.18.4 自动继续](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/session/compaction.ts)。
支持压缩 marker 的新版本仍通过现有
[官方 ACP marker](https://dev.opencode.ai/v2/docs/cli/acp/#compaction-marker) 映射生命周期。

## 验证

验证范围为 provider crate 的 OpenCode 代码及其直接相关行为，默认 features。
没有运行全 workspace 测试。截图所用主机只读排查；真实模型服务未重新请求，远端未部署，其他平台未验证。

- 初次修复的 OpenCode 离线相关测试：82 passed、25 ignored；真实 1.18.4 用量／停止原因：3 passed。
- 真实手动／自动压缩：2 passed，并保持原测试并行度复核。首次执行的自动压缩在关闭后
  读取历史处返回过一次 `Failed`，随后重复验证未复现；该次失败原因尚未确认，不能声称已消除。
- provider 全 targets 的严格 Clippy、Rust 格式、7 个 Markdown 格式及 388 个文档链接检查通过。
  Python 夹具仅做语法编译检查，不生成仓库内字节码。
- `cargo build --locked --workspace` 通过；未改动的 filesystem 存在 `fetch_update` 弃用警告。
  全 workspace 严格 Clippy 在未改动的 host-link / model 测试中遇到 8 个
  `clippy::assert_is_empty` 错误，未扩大修改范围。
- 为运行 provider 严格 Clippy，同步将已有 Pi 测试的空字符串断言改为 `assert_eq!`，
  无行为修改；对应 admission 模块 8 项测试通过。
- 并发回归曾暴露原有启动测试在释放门锁后仍只给正常启动 150 ms 的时序问题；
  恢复阶段已还原正常启动预算，保持原有短时门锁超时断言与测试并行度。

```sh
cargo test --locked -p provider --lib local::opencode
AIT_TEST_OPENCODE_BIN=/usr/local/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed::compaction -- --ignored
AIT_TEST_OPENCODE_BIN=/usr/local/bin/opencode cargo test --locked -p provider --lib local::opencode::tests::installed::usage -- --ignored
cargo test --locked -p provider --lib local::pi::tests::admission
cargo build --locked --workspace
cargo fmt --all --check
cargo clippy --locked -p provider --all-targets -- -D warnings
# 已执行但受基线测试 lint 阻挡
cargo clippy --locked --workspace --all-targets -- -D warnings
node_modules/.bin/oxfmt --check docs/README.md docs/reports/README.md docs/decisions/providers/adr-115-opencode-acp-provider.md docs/decisions/providers/adr-121-acp-turn-history-reconciliation.md docs/decisions/providers/adr-150-provider-compaction-lifecycle.md docs/decisions/providers/adr-159-opencode-acp-native-observation.md docs/reports/providers/opencode-auto-compaction-2026-10-19.md
node scripts/check-docs.mjs
```

离线回归覆盖同轮两次压缩继续、原生历史的完整结算、后续输入、关闭与恢复、
持久化身份预算边界和无 prompt 重发。真实 CLI 回归验证自动摘要进入后续模型请求，
并覆盖原生继续消息关联、回放与恢复后继续对话。
新增终态回归覆盖正常完成、取消、输出上限、轮次请求上限、拒绝，以及持久错误提示后用户继续。
未知的 ACP `stopReason` 仍使连接失效，不当作成功，也不重发输入；
这与上游内部 `finish=unknown` 被转换成合法 `end_turn` 是不同情况。
用量回归覆盖非零计数、下降的计数、零计数，以及真实 CLI 在取消占位消息后的零用量。
认证只读 HTTP/SSE 回归覆盖实际缓存 token、分页、循环游标、会话隔离、重复 marker 和历史边界。
真实 CLI 回归还要求在模型第二步仍等待时看到第一步用量，并要求原生 unknown finish 显示错误后可继续。

2.x 后续复查发现独立 compaction 的轮次映射问题，已沿 ACP 身份修复；
追加测试与最终结果见 [2.x ACP 复查](opencode-2x-acp-2026-10-19.md)。

## Test coverage

**not measured**：本次未请求覆盖率测量；测试通过数量不代表覆盖率。
如需测量，使用 `cargo llvm-cov` 限定 provider crate 与上述 OpenCode 回归过滤器。
没有生成新的覆盖率产物，也不引用历史覆盖率作为本次结果。
