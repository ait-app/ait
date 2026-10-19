# Codex 长输入与连接失败排查

- 日期：2026-10-19。
- 源码基线：`c517756ad06488c99140f965832478d844ddbb88` 加本次修改。
- 平台：gpu-desktop，Linux x86_64；原生 Codex 0.162.1。
- 决策：[ADR-144](../../decisions/providers/adr-144-provider-error-reporting.md)、
  [ADR-162](../../decisions/providers/adr-162-codex-error-details.md)。

与另外两个 session 汇总并合入最新 main 后的验证见
[PR 整合报告](../daemon/error-diagnostics-and-recovery-pr-2026-10-19.md)。

## 已确认的失败来源

用户无法回忆截图的准确会话与时间，授权按 gpu-desktop 上相同提示的失败记录排查。
只读对照 Ait journal、持久 Agent 状态、Codex 原生 `thread_turns.error_json` 与 rollout；
没有提交真实模型请求、修改原生数据或配置，也没有部署或重启服务。

同一原生线程 `01a14c98-3f53-77b2-832c-e31728e21de4` 有以下失败，时间均为 UTC：

| 时间                | Codex 原生终态错误                                       | Ait 原提示 |
| ------------------- | -------------------------------------------------------- | ---------- |
| 2026-10-18 08:54:42 | `httpConnectionFailed`，HTTP 403，`insufficient balance` | 检查网络   |
| 2026-10-18 12:53:35 | `other`，`stream closed before response.completed`       | 检查网络   |
| 2026-10-18 13:07:05 | `other`，`stream closed before response.completed`       | 检查网络   |

第一类由上游模型服务返回余额不足。另有其他线程记录过 Cloudflare 拒绝访问的 HTTP 403。
第二类为 Codex 已接纳输入之后，其模型响应流没有收到完成事件；原生重试和 HTTP 回退
均未恢复，最后以 `turn/completed.status=failed` 通知 Ait。
两次断流分别在新轮次开始约 26.5 秒、25.3 秒后结束；Ait journal 的来源为原生
`notification`／`result`，没有把这些事件记为本地请求超时或事件队列溢出。

其中一次用户消息约 4,182 字符，另一次仅 8 字符，不能归因为单条输入字节数越界。
此前原生 rollout 已有 18 次 compaction；最近有效用量约为 79,954 / 258,400 token。
该计数可能沿用前次请求，不代表失败请求的实际 token 数；它和原生终态都没有提供
上下文超限的证据，也不能用于证明任意长输入都不会遇到上游限制。

这些记录确认失败发生在 Codex 与上游服务的请求／响应链路，Ait 没有主动截断对应请求。
现有数据无法进一步区分断流发生在模型服务、代理、网络或 Codex 流处理中的哪一环，
也不能把它认定为已证实的 Codex 程序缺陷。无法唯一关联原截图，不对全部类似提示作相同归因。

官方 [Codex app-server 错误协议](https://learn.chatgpt.com/docs/app-server#errors)
说明 `HttpConnectionFailed` 可以包装上游 4xx/5xx，而不只表示客户端网络故障。
`turn/completed` 的 `error` 与 HTTP 状态是归因依据；Ait 不应仅根据包装名称忽略具体原因。

## 源码修复

`provider::local::diagnostics` 之前把 `httpConnectionFailed` 直接定为 `Network`，
不再检查 HTTP 403 或余额原因；文字匹配也缺少 `insufficient balance`。
这是 Ait 已确认的诊断缺陷，导致用户把上游额度／拒绝访问误认为本地断网。

- 识别 `insufficient balance`；宽泛连接包装允许由明确原因或 HTTP 状态细化。
- 保留结构化上下文／额度等具体分类的优先级，不让笼统连接文本覆盖它们。
- 将缺少原生完成事件的断流单独分类为 `incomplete_response`，提示响应未完成与检查上游服务。
- 聚合日志仍只保存固定分类、数字状态和来源；界面按 ADR-162 显示有界脱敏的原生错误说明。

改动沿用现有 adapter 与诊断 port，不引入新的能力边界、RPC 或数据库格式。
原生重试仍由 Codex 管理，Ait 不自动重发用户输入，也不改变用户的 Provider 配置。
本次修复使失败原因准确可见，不声称已经消除上游余额不足或响应流中断。

## 聊天内展示具体原生错误

用户进一步要求正确暴露错误。仅修正分类仍然丢失了原生说明，因此 Codex adapter
现在通过已有 `failure_message()` 提供实际原因、数字 HTTP／RPC 状态和已知重试结果。
例如，观察到原生重试后，上游余额不足显示为：

```text
Codex (HTTP 403): unexpected status 403 Forbidden: insufficient balance
Codex retries did not recover the request.
```

上游 `request body too large` 等未专门分类的原因也会显示，包含原生 HTTP 413 状态。
串在 `message` 中的 API 错误 JSON 选择内层 `error.message`，避免把整个响应或额外诊断
复制到界面。没有有效说明时才回退固定分类。

消息最多检查 32 KiB、原因保留 3072 字节；按 UTF-8 边界截断并明确标记。
已知凭据字段、Bearer、常见 API token／JWT 被替换；URL 仅保留服务 origin，
去掉认证信息、路径、query 和 fragment。`additionalDetails`、stderr 和请求载荷不参与展示。
这不是任意文本的完全匿名化保证；用户要求的原生说明与其他诊断载荷仍有明确边界。

同一说明进入聊天 Timeline、持久 `lastError` 和 `turn_failed.error`，现有错误日志复制入口
也读取它。关闭 native transport 前保留说明，新输入清除旧状态；原生重试恢复成功不显示失败。
不增加 wire 字段或改变客户端展示流程，不回写原生 Codex 历史。旧的泛化错误条目不会被猜测性重写，
运行中的安装包也未部署或重启。

## 验证

新增 HTTP 包装回归在修复前失败：余额不足被分类为 `Network`，期望为 `Quota`。
第一阶段分类修复的定向验证：

- 公共诊断：9 passed。
- Codex adapter：180 passed、1 ignored；ignored 需要真实认证及模型请求。
- 持久失败提示与清理重试去重：1 passed。
- provider 全 targets 严格 Clippy 通过；Python 夹具通过 AST 语法检查。
- workspace 编译检查通过；未修改的 filesystem 有既有 `fetch_update` 弃用警告。
- 修改文件的 Rust／Markdown 格式检查及 390 份 Markdown 的本地链接检查通过。

第一阶段合计 190 项测试通过；没有运行完整 Rust workspace 测试。
新增协议夹具覆盖余额不足、访问拒绝、缺少流完成事件，分别经过错误通知＋原生重试状态
和只含终态 error 的路径。验证输入只发送一次，后续新输入清除旧诊断并可正常完成。
既有 Codex 定向测试还覆盖 8 MiB 历史、较大图片事件、出站预算、压缩生命周期与请求超时。

第二阶段新增回归在接入前失败：`failure_message()` 没有原生错误说明。
补充验证包含凭据／URL 脱敏、已知重试状态、超长多字节错误、序列化 API 错误、
原生 RPC 413，以及 Codex 错误通知与失败终态两条路径。
端到端夹具检查同一说明进入聊天、终态事件和持久快照，原生历史刷新及重新打开存储后
仍保留原错误；关闭 transport 不丢失说明，后续输入清除旧状态。

第二阶段 Codex 定向测试 **188 passed、1 ignored**，公共诊断 **9 passed**，
持久失败去重 **1 passed**，合计 **198 项不同的测试通过**。
ignored 仍为需要真实认证及模型请求的用例，没有扩大到完整 workspace 测试。
最终 provider 全 targets 严格 Clippy、workspace 编译检查和修改文件格式检查均通过；
transport 最后清理后另复核 **24 passed**，不重复计入上述 198 项。
393 份 Markdown 的本地链接检查通过。workspace 检查仍只有上述未修改文件的既有弃用警告。

```sh
cargo test --locked -p provider --lib local::codex --quiet
cargo test --locked -p provider --lib local::diagnostics::tests --quiet
cargo test --locked -p provider --lib service::agent_manager::tests::paseo::classified_failures_are_durable_visible_and_not_duplicated_on_cleanup_retry -- --exact
cargo clippy --locked -p provider --all-targets -- -D warnings
cargo test --locked -p provider --lib local::codex::transport::tests --quiet
cargo check --locked --workspace
rustfmt --edition 2024 --check crates/provider/src/local/codex.rs crates/provider/src/local/codex/transport.rs crates/provider/src/local/codex/failure.rs crates/provider/src/local/codex/tests/diagnostics.rs crates/provider/src/local/codex/permissions/tests/paseo.rs
node_modules/.bin/oxfmt --check docs/README.md docs/decisions/README.md docs/decisions/providers/adr-144-provider-error-reporting.md docs/decisions/providers/adr-162-codex-error-details.md docs/reports/providers/codex-upstream-errors-2026-10-19.md
node scripts/check-docs.mjs
```

## Test coverage

**not measured**：用户未请求覆盖率测量；通过数量不代表行覆盖率。
没有生成覆盖率产物，也不引用旧报告作为本次结果。
后续如需测量，使用 `cargo llvm-cov` 限定 provider crate 和上述定向过滤器。
