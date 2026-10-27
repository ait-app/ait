# ADR-193：Codex 原生策略、事件与恢复兼容

- 状态：Accepted
- 日期：2026-10-27
- 修订：ADR-041 的 RPC 等待事件预算；延续 ADR-046、ADR-052、ADR-177、ADR-179、ADR-191、ADR-192。
- 对照：Paseo `4f9d9d6de2236486ecc2ad886a2c37f4b92271e0` 与 Codex CLI 0.164.0。

## 背景

等待 RPC 回执期间，正常的流式通知可以超过 128 条；直接失败会中断合法任务，无界内存队列
又会扩大宿主内存风险。与此同时，原生完成通知、审批回调和本机配置具有独立的生命周期，
不能用宿主预设重新推断其结果。新版 Codex 的子 Agent 活动和回退参数也需要适配。

## 决策

1. Codex adapter 保留有界接收通道和至多 128 条等待回执事件的内存队列，溢出部分写入
   adapter 拥有的私有临时文件。磁盘读写交给 blocking task，批量回放保持原始顺序；读取
   未完成时不能越过旧事件消费新通知。文件在队列耗尽、关闭或丢弃时释放。存储失败或
   RPC 超时仍失败关闭，不能丢掉事件后宣称成功。语音关闭等待复用同一队列，保留普通文本、
   审批与 canonical speech 的顺序，并丢弃上一代媒体通知。暂存文件不属于原生历史或宿主持久状态。
2. `turn/interrupt` 仅将原生 `-32600 / no active turn to interrupt` 视为幂等成功，随后由
   已排队的原生终态结算。其他错误维持原拒绝或不确定接纳规则，不重放用户输入。
3. adapter 读取 `config/read` 的工作区权限，并保存 `thread/start`、`thread/resume` 返回的
   有效 sandbox。同一类型沿用原生策略；切换类型时合并继承设置与明确的 provider options。
   UI 权限模式仍沿用现有预设，不以默认字段覆盖原生的额外目录、网络或临时目录限制。
4. 交互恢复只在当前 thread 的精确归档拒绝之后执行 `thread/unarchive` 并重试 resume，
   允许另一客户端已经取消归档的明确竞态。只读历史访问不取消归档；无关失败不重试。
   用于判定的原生错误仅在 adapter 内以有界 `SecretString` 保留，Debug 不输出原文。
5. 斜杠名称必须匹配实际目录后才展开；其他文本原样发送。自定义 prompt 读取当前会话
   启动环境中的 `CODEX_HOME` / `HOME`，无 shell 求值。非法单个文件不会隐藏其他命令。
6. 同时接受 `subAgentActivity` 和旧 `collabAgentToolCall`，仅由已证明的原生父子关系登记
   子会话。父活动项关联子会话；子会话状态及正文继续通过独立 descriptor / timeline 发布。
   终端输入作为显示事件投影，进程标签有界保留并可跨 turn 使用；PTY 执行仍由 Codex 拥有。
7. `thread/compacted` 与 compaction item 完成按事件顺序配对去重；缺少最终 item 时，在 turn
   结算前关闭 loading。手动压缩完成释放前台准入，子会话使用同一规则并保持事件归属。
   新旧用户问题回调都可回答，原生非阻塞标志不能使会话失败。
8. 回退对每个目标 turn 使用排他的 `beforeTurnId`，并传递线程配置；旧版本忽略该字段时，
   只在新 fork 上 rollback 所需后缀。验证保留前缀和原会话不变之后，宿主才替换句柄。
9. 保留非空原生 reasoning 内容、已解析的默认模型及 systemError thread 的已完成历史。
   辅助摘要只因当前 turn 的不可重试错误失败，不因原生重试或旧 turn 错误提前退出。
   异步提问作为独立交互项处理，不覆盖本轮已完成的 assistant 回答。

## 后果

事件突发可使用临时磁盘，内存中的待回执条目数量保持有界；临时磁盘不可用时仍会明确失败。
原生会话、工具、认证和历史的所有权不变，新增状态仅位于 provider adapter 内，不增加 RPC、
domain 实体或数据库迁移。文字与 timeline 的既有资源预算继续生效。

验证范围和对照证据见 [Codex / Paseo 兼容验证报告](../../reports/providers/codex-paseo-compatibility-2026-10-27.md)。

## Test coverage

**not measured**：用户未请求覆盖率测量。定向回归、原生离线集成与 lint / 编译结果分别记录在
验证报告中；若后续测量，限定 provider 的 Codex、工具投影和直接相关的接纳、订阅路径。
