# ADR-206：OpenCode 的 ACP 子 Agent 观测与历史归属

- 状态：Accepted，2026-10-31。
- 范围：`provider::local::opencode`；补充 [ADR-115](adr-115-opencode-acp-provider.md)。

## 决策

1. ACP 继续拥有执行、审批、取消和完整历史。协商客户端能力
   `_meta["opencode/child-session-updates"]`，消费原生
   `opencode/session/child_update` 的创建、运行、完成、失败和中断事实。
   同时识别旧式 `session/update._meta["opencode/child-session"]`，将子输出分流到
   现有 `SubagentEvent`，不参与主会话文本、用量或完成状态。
2. 按 root、直接 parent 和 depth 验证归属，子先于父到达时使用有界缓冲。
   保留嵌套树，拒绝环、身份冲突、错误层级和超预算数据。
   ACP 转发所加的工具 ID／标题前缀在子投影中移除，实时与历史使用相同原生身份。
3. 原生 task／subagent 工具的结构化 `rawOutput.metadata.sessionId` 或 `sessionID`
   是历史关联证据；不解析提示词、正文、XML 标签或工具标题中的会话 ID。
   通过 `session/list` 与 `session/load` 有界遍历关系，支持无 Ait 缓存时的发现。
   子会话 locator 保存直接父身份；读取独立子历史时重新回放父工具结果验证归属。
4. 一棵树最多 256 个子会话，实时层级最多 32；原生分页、发现总期限、未归属事件数和
   字节数均有界。子 Agent 查询不创建、删除或提交原生会话输入，不读取原生数据库。
5. 主 prompt 结束后继续消费后台子事件和原生审批。子生命周期不会重复结算主回合。
   历史 task 的后台运行状态不能覆盖宿主已经观测并持久化的较新终态；恢复的关系用于
   归属验证，只有新实时观测才作为 live 状态上报。
6. 不具备子事件的旧 ACP 只能在结构化工具结果出现后发现关系，并读取子历史；
   不伪造其缺失的创建、内部执行和终态事件。历史协议未提供子会话时间或 token 时不估算。
   本变更不扩大 ADR-159 的 1.18 HTTP 观测边界，也不增加另一个执行通道。

## 协议依据与验证

- [官方 ACP 子会话说明](https://opencode.ai/v2/docs/cli/acp/#child-sessions)。
- [2.1.4 子会话事件](https://github.com/anomalyco/opencode/blob/v2.1.4/packages/cli/src/acp/child.ts)、
  [能力协商](https://github.com/anomalyco/opencode/blob/v2.1.4/packages/cli/src/acp/capabilities.ts)、
  [原生工具元数据](https://github.com/anomalyco/opencode/blob/v2.1.4/packages/core/src/tool/plugin/subagent.ts)。
- 测试位于 provider 内：离线 ACP 回归覆盖实时分流、嵌套、历史发现、伪造归属拒绝和后台审批；
  opt-in 原生测试使用隔离 XDG 和回环模拟模型验证 OpenCode 的实际协议，不需要用户认证。
