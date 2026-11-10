---
id: ADR-236
status: accepted
date: 2026-10-31
amends:
  - providers/adr-235-provider-native-command-ownership.md
paths:
  - crates/provider
  - packages/protocol
  - apps/mobile/src/components
---

# ADR-236：原生控制结果使用独立时间线卡片

状态：接受。日期：2026-10-31。延续 [ADR-235](adr-235-provider-native-command-ownership.md)。

## 背景

真实桌面操作中，DSH 的 goal、plan、permission 等命令结果被投影为模型回答。
目标的多行状态、内部 activation 字段和命令用法因此铺满聊天区，缺少可操作入口。
这也混淆了控制事务和模型回合，历史恢复还丢失了命令输入。

## 决策

- 共享时间线增加 `provider_command`：包含稳定原生身份、命令名、成功或失败、原始输出，
  可附带结构化 goal 事实和 Provider 提供的操作；内部存储与当前界面将命令结果和模型回答分开。
- DSH adapter 从 `command/run`、`command/done`、`goal/change` 和 goal round 的原生
  journal 派生数据。命令名按原生 commandId 关联；目标状态直接读取原生结构，不能
  解析英文输出、猜测状态或在客户端重写插件业务规则。live 与完整 history 共用投影。
- 原生 `command/run` 恢复用户命令和原始参数；`recordInput=false` 时尊重省略的输入，
  不从其他事件或客户端草稿重建可能敏感的参数。
- 同一次目标命令的状态变更与结果合为一张卡片；自主完成、阻塞等无命令的变更也发布卡片。
  轮次表示执行次数及上限，不作为目标完成百分比。进程内 activation 不冒充持久状态。
- 客户端展示目标、状态标签、轮次及操作，原始输出收进详情。普通命令使用紧凑的结果卡片。
  暂停、继续、编辑和清除通过原有消息 admission 执行 Provider 原生命令，复用重试身份和
  提交状态，不新增直接访问原生 Host 的写入通道。
- 历史卡片、归档会话、分离的历史窗口及尚未完成权威恢复的窗口不提供活动操作。
  卡片身份、结构化状态和操作也进入客户端持久缓存，刷新后仍按原生事实展示。
- 传输出口始终使用已有 `assistant_message` 变体：`text` 提供旧客户端可读的结果，
  可选 `providerCommand` 元数据携带身份、状态、目标、动作与文件信息，避免新增判别成员破坏旧客户端。
  元数据不重复保存正文；`messageId` 使用稳定命令身份。旧 schema 忽略可选字段，新客户端还原独立卡片。
  实时事件、子 Agent 事件和历史分页共用同一编码；持久投影缓存解码时恢复内部类型，不改变游标、
  源覆盖范围或分组身份。升级不依赖“同版本共同发布”的假设，也不提高最低连接协议版本。
  现有安装的第三方 CLI 不修改。当前首先用于 DSH，其他 Provider 仅在有相应原生控制事实时接入。
- DSH 恢复保留 native handle，通过公开 `session/selectModel` 确认当前保存的模型并
  恢复 Agent；`session/create` 的 sessionId 是空会话复用提示，不能用于已有对话。
  首先验证只读快照的 cwd 和身份再激活；模型目录或配置不可用时报告恢复失败。

## 验证

共享 fixture 贯穿 Rust 投影、TypeScript schema、生成的 WebSocket validator 和桌面组件。
另以 `850e0075` 的真实 schema 验证实时消息和整页历史均可读取。
定向测试覆盖自主完成、清除、暂停后的合法按钮、原始输出折叠、编辑参数保留及缓存恢复。
桌面实测结果及仍未完成的原生功能分别记录在 [适配报告](../../archive/reports/providers/dsh-opencode-capability-adaptation-2026-10-30.md)。

## Test coverage

**not measured**：未请求覆盖率；测试执行结果与真实桌面验收单独记录，不以通过数量替代覆盖率。
