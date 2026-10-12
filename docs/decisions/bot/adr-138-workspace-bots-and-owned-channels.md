# ADR-138：Workspace Bot 与独占 Channel

- 状态：接受
- 日期：2026-10-12
- 修订：为 [ADR-104](../workspace/adr-104-filesystem-model-collaboration.md) 所述功能
  crate 独立依赖约定增加 `bot → provider` 单向边；其余能力边界保持原有约定。

## 背景

Workspace 需要长期存在的对话机器人，既能从 Ait 的独立 tab 对话，也能接收 Telegram、
飞书等外部消息。Bot 使用现有 Provider 原生会话执行，session 在普通 Agent 目录中隐藏。
一个 Bot 同时服务多个对象，各自需要独立上下文；长期记忆依托 Workspace 目录保存。

## 决策

1. 新增 Rust `bot` crate，拥有 Bot 生命周期、Channel、Conversation、消息协调、协议和
   平台 adapters。生产内部依赖为 `domain`、`model`、`provider`；persistence 仅为测试依赖。
   API 与 daemon 安装完整 Bot 服务，daemon 注入具体文件存储及 Workspace registry。
2. 每个 Bot 必须属于一个有效 Workspace。每个 Channel 只属于一个 Bot，入站目标由
   Channel 身份直接确定。第一版同一个外部接收账号在同一 daemon 内只能属于一个活动
   Channel，避免重复接收。平台 adapter 实现可复用，账号实例和业务归属不共享。
3. Conversation 以 Channel、对话位置、发送者和线程区分，默认隔离群聊中不同发送者的上下文。
   当前隐藏 session 由 Conversation 管理，重置增加 generation。模型配置变更在新
   generation 生效，已经接纳的输入保留对应配置与明确回复目标。
4. Provider 提供受限的内部执行接口，复用现有会话所有权、恢复、输入回执、历史和取消。
   隐藏标志与 Bot 所有权在首次登记前确定，不能通过公开 Agent 创建参数伪造。Provider
   不依赖 bot，不负责外部通信。Bot 不实现第二套模型执行循环。
5. Ait 内置 Channel 随 Bot 创建。客户端 tab 目标使用稳定 `botId`，通过 `bot.*`
   方法发送和观察 Conversation，底层 Agent ID 不作为 UI 对话身份。关闭 tab 不停止 Bot。
   当前共享 daemon 凭据下，内置 Channel 使用稳定 owner 对话；连接 ID 不充当用户身份。
6. Telegram 首先使用长轮询，飞书使用自建应用长连接。平台事件在持久接纳后确认，模型
   执行和发送不占用平台确认路径。外部 Channel 第一版发送最终文本，Ait 可观察执行状态。
7. 配置、人格、会话映射、输入与投递状态保存于 daemon 数据目录。首版共享记忆保存在
   `<cwd>/.ait/bots/<sha256(workspace_id)>/<bot_id>/MEMORY.md`；对方私有记忆和摘要可后续扩展。
   复用本地文件持久化，无新增外部数据库、向量库、消息队列或 Node/Go 辅助进程。
8. 凭据只保存环境变量引用，由 daemon 注入解析。外部内容不扩大工具权限，默认允许列表
   和群聊提及触发；审批通过 Ait Bot 接口处理。目录分层本身不承诺文件工具的安全隔离。
9. Email 作为后续可选 Channel：通用方案为 IMAP 收件、SMTP 发件，按邮件标识组织线程，
   控制自动回复循环。首次交付不宣称已支持 Email，也不提前引入邮件协议依赖。

## 生命周期与恢复

同一 Conversation 串行，不同 Conversation 受全局预算限制并发。隐藏 session 空闲时可释放
原生资源，Bot 和记忆继续存在。已接纳输入持久化后才返回成功；完成执行与待发送内容关联，
重启继续尚未发送的最终回复；中断的执行与正在发送的回复记录 uncertain，需显式处理，
避免盲目重复执行。没有平台幂等保证时不承诺 exactly-once 投递。

暂停 Bot 停止接纳并取消工作，保留可恢复的隐藏会话身份；归档退役会话、停用 Channels 并保留历史。Workspace 归档或目录缺失会停止
执行。worktree 删除需要处理目录中的记忆，不能静默重建空目录后继续服务。

## 后果与验证

实施同步 Cargo 声明、daemon 依赖守卫、API 方法安装与停机及 Workspace tab 接入契约。
前端 tab 渲染与流式订阅单独接入，当前通过持久查询观察 Bot。功能细节及实施范围见 [Bot 后端设计](../../plans/bot-backend.md)。

定向验证覆盖独占 Channel、会话隔离、创建即隐藏、重复输入、重启恢复、确定目标回复、
平台确认顺序、凭据脱敏、暂停与归档。运行受影响检查，不执行完整 Workspace 测试。
