---
id: ADR-307
status: accepted
date: 2026-11-25
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
  - bot/adr-306-shared-bot-context-and-chat-reception.md
paths:
  - crates/bot
---

# ADR-307：Bot 重启换代与有限回执排空

## 背景

共享原生上下文使一个执行结果不明的轮次阻塞整个 Bot。无人值守的 daemon 重启后，要求用户先重置才能处理新告警会使所有入口持续停摆。
平台发送与执行状态是两种独立的不确定性；停止时立即丢弃发送 future 还会把原本能够确认的回执变成投递不确定。

## 决策

启动时先将仍在执行的轮次标为 `uncertain`。存在当前 generation 执行不确定记录的 Bot 自动换用新的共享原生上下文，
再推进这些来源会话的 generation。旧记录、输入去重和任务来源保留；不重新发送旧的执行输入，不把不确定操作当作失败重做。
仅在 Provider 从未收到的 `queued` 输入继续按原有 Bot 顺序处理。长期记忆、工单与历史读取仍可用，旧原生上下文的临时记忆不带入新上下文。

共享上下文的新身份先落盘，再推进不确定来源；若中间崩溃，下次启动允许再次换代，不允许恢复旧原生身份。
未执行队列的 generation 在启动时与来源记录对齐，因此换代成功、队列更新未完成时崩溃不会丢弃已接纳输入。
暂停的 Bot 可以换代，但恢复前不执行。运行期间新出现的不确定性仍阻塞当前 Bot，直到显式重置或下次启动。

投递 `uncertain` 不触发执行上下文换代，也不重发。尚未尝试的回执保留在 outbox；中断唤醒仍按通知日志的既有去重规则重新唤醒，
启动恢复后的原来源可继续使用，不再仅因旧 generation 的执行不确定而回退到内置聊天。

停止时关闭入站接纳并取消原生执行。每个 Channel 只允许正在进行的回执发送继续最多 10 秒；不领取下一条回执。
已确认成功落为 `delivered`，失败或超时落为投递 `uncertain`。worker 在 Channel 和原生任务结束前继续处理持久化结果，
随后关闭命令通道。该窗口包含在 daemon 现有的关闭期限内，不新增信号处理器或延长宿主期限。

## 后果与兼容

不确定记录不再永久阻塞重启后的新消息，但自动换代不能确认旧操作是否产生副作用。人工或模型重试前仍须核查旧记录及任务状态。
该规则不提供多副本同时执行、分布式领导者选举或操作幂等保证。

本变更不新增 RPC、配置或持久化字段。旧版仍可读取记录，但不提供上述启动续接和有限排空行为。

接收器兼容飞书或 Lark 的应用机器人身份并按自身 Open ID 与 App ID 过滤，不替代平台事件权限。
官方接收事件与查询消息的发送者结构应分别理解，见[接收事件](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive.md)
和[查询消息内容](https://open.feishu.cn/document/server-docs/im-v1/message-content-description/message_content.md)。
