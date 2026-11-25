---
id: ADR-305
status: accepted
date: 2026-11-25
amends:
  - clients/adr-218-hub-feature-negotiation.md
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
  - bot/adr-280-bot-recall-reads-and-spill.md
  - bot/adr-290-open-channel-senders.md
paths:
  - crates/domain/src/bot.rs
  - crates/bot
  - packages/protocol/src/bot
  - packages/client/src/protocol
  - crates/api/src/connection/single.rs
  - apps/mobile/src/bots
---

# ADR-305：Bot 共享执行上下文与外部会话接收策略

## 背景

同一 Bot 在多个群聊、私聊与内置入口中持续处理同一工作。按参与者和话题隔离原生模型历史会使跨入口追问失去上下文；将监听配置放在 Channel 上又会同时改变账号加入的所有群。

## 决策

Bot 持有一个共享原生执行上下文。现有 Conversation 保留为来源、历史与回执路由；Channel 仍代表平台账号。接收策略按 Channel 与平台会话标识保存，参与者及话题不参与配置寻址。群默认仅处理提及，私聊视为直接呼叫；飞书和 Lark 的旧 allowGroups 字段只保留格式兼容，不再作为跨群总开关；设置只能由 Ait 客户端修改。

Bot 管理轮次按持久化的 Bot 顺序串行执行，子 Agent 仍可并发。作用域凭据在每轮开始前绑定本轮来源并撤销旧凭据，在结束时撤销；工单和子 Agent 复制该来源。共享上下文不等于把外部调用者变成内置所有者：管理者可回读同一 Bot 的所有入口与工单、操作这些工单的 Agent，子树保持来源和工单限制。

普通回复跟随当前输入，异步通知继续按任务来源分组，不广播。被动监听输入可以静默结束。群内人员和其他机器人的消息统一解析，按平台消息标识去重。飞书平台权限仍决定能够推送哪些事件，Ait 设置仅决定是否将事件接纳为模型输入。

重置共享上下文使用独立的 Bot 级接口，旧单对话重置不再发布。迁移关闭旧孤立原生会话，保留历史和未完成任务供读取，不重放输入或自动重做结果不确定的操作。

## 后果与兼容

加入同一 Bot 的所有入口属于同一个上下文共享范围，群和私聊之间不提供内容隔离。不同 Bot 仍隔离。已有工具组和工作区授权继续有效；该变更不建立操作系统或凭据沙箱。

Catalog 新增可省略的共享上下文索引和会话策略，Turn 新增可省略的顺序、发送者类型与被动输入标记。没有新数据时默认字段不写出。写出新数据后旧版的 deny_unknown_fields 无法加载，降级需恢复升级前整个 workspace-bots 目录的备份，不能只删除字段后重放任务。客户端按方法集合协商 workspaceBotChatReception 与 workspaceBotSharedContext，不通过 Bot 实例字段推断协议能力。四个旧 SDK 分拆连接均已满 64 capability；三个新增配置方法使用独立分拆连接 4，保留每连接 64 capability 上限，单连接传输仍使用一个 socket。daemon 单连接模式把配置调用分配至文件执行 lane，与 socket 编号无关。旧单会话重置方法仍保留客户端协议解析，但共享上下文主机不再发布它。

每个会话的接收设置在 Settings 的滚动区域内呈现；读取遵循 interactive 门控，保存按行串行并使用响应更新缓存。共享重置会使所有来源的会话列表及历史缓存失效。

依据：[飞书接收消息](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive.md)、[接收消息内容结构](https://open.feishu.cn/document/server-docs/im-v1/message-content-description/message_content.md)。
