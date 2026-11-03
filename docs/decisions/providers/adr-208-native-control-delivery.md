# ADR-208：原生控制的延迟确认与文件交付

状态：接受。日期：2026-11-02。扩展 [ADR-205](adr-205-provider-native-command-ownership.md)
和 [ADR-206](adr-206-native-control-result-cards.md)。

## 背景

DSH 在附件校验前记录 `command/run`，原生处理器还能继续提问。以 run 作为最终发送成功
会丢失失败草稿；在同一执行 lane 等待整个 HTTP 请求又会阻塞问题回答。原生 `/export`
只应答下载请求，实际 ZIP 需要通过同一 Host 的认证二进制接口读取。

## 决策

- Session port 可交出拥有独立生命周期的最终控制确认 future。application 保留 input receipt
  的 claimed 状态，确认成功、明确拒绝和不确定失败分别落为 accepted、rejected、uncertain。
  业务原生命令仍由 adapter 执行，不在 application 理解 DSH 语法。
- RPC 的最终应答可以延迟，但会话 lane 继续轮询和处理审批、提问、取消与其他请求。每个
  延迟应答保留原 admission permit，并归所属 lane 的有界原生任务与 JoinSet 生命周期管理。
  关闭原生会话先中止未完成操作，再收敛应答任务；客户端断开不触发自动重发。
- 首条带附件命令同样经过延迟确认；Agent 可先发布 agent_ready，原生拒绝不得将 creation
  写成 completed。普通消息和不需要延迟确认的控制保留原路径。
- DSH 执行前检查命令声明的附件能力及全部附件形状。图片使用原生 encoded-image；文件
  必须是可读取、大小匹配的普通 uploaded_file，最大 256 MiB，流式上传到 receiving Agent
  取得 staged receipt。文本上下文、forge 和 review 对象没有对应的原生命令附件语义时拒绝，
  不将文件路径或文本说明冒充 receipt。任何上传失败都不提交剩余的命令。
- 只有目录明确带第一方 definitionId 的 DSH 导出命令才触发 ZIP 交付；原生参数校验及
  命令结果仍有优先权，同名插件不被截获。成功后从同一认证 Host 请求 session tree，包含
  descendants 和附件。ZIP 流最多 1 GiB，失败、超限或取消不发布部分文件。
- 导出文件保存在 daemon 的私有 provider artifact 目录。用 native command identity 的哈希
  定位文件，原有 command card 增加 file 元数据和下载按钮，历史回放可重建同一条目。
  不增加虚构模型消息，不将 ZIP、Cookie 或 bearer URL 放进历史，也不向项目目录写入文件。
- 客户端复用现有单次下载票据和跨桌面、Web、移动端下载管道；无需直连 DSH 的 loopback
  地址。只有文件实际交付后才展示下载入口。

## 后果

本协议允许有附件的原生控制完成异步交互，同时保留草稿失败恢复。原生 handler error 与
模型 turn 的最终回答保持独立；明确拒绝允许用户重试，accepted receipt 防止重复提交。
文件失去本地工件后，原生历史仍可读取，下载入口不再显示；不会为读取历史重新执行导出。

## Test coverage

**not measured**：用户未请求覆盖率测量。验证范围为 owning DSH adapter、发送和创建、
执行 lane 与协议和下载卡片；测试执行结果记录在能力适配报告中。
