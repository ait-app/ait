---
id: ADR-233
status: accepted
date: 2026-11-09
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-213-single-workspace-manager-bot.md
  - daemon/adr-045-remove-hub-chat-loop.md
  - clients/adr-137-remove-retired-feature-protocol.md
paths:
  - crates/bot
  - bins/daemon
---

# ADR-233：Ait 中心作为 Workspace Bot 的 Hub Channel

- 状态说明：已实现（daemon 侧；客户端添加 Hub Channel 的界面待后续）
- 关系：扩展 [ADR-139](adr-139-workspace-bots-and-owned-channels.md) 的 Channel 传输与 [ADR-213](adr-213-single-workspace-manager-bot.md) 的 v2 接口；部分推翻 [ADR-045](../daemon/adr-045-remove-hub-chat-loop.md) 与 [ADR-137](../clients/adr-137-remove-retired-feature-protocol.md) 对 Hub 的移除：不恢复上游 Paseo 的 `hub.management.*` / `hub.execution.*`，改为在 [ADR-180](../daemon/adr-180-hub-serve-endpoint.md) 的 serve 链路上接受中心会话，准入方式沿用 [ADR-208](../daemon/adr-208-workspace-share-guests.md) 的访客会话。中心侧见 ait-server `docs/automation.md`。

## 背景

用户希望在飞书、Slack、Telegram 中 @机器人，由中心把消息交给本账号某台 Host 处理，并把结果回复到原会话。上一版设计让中心直接创建 Agent，并通过公网 MCP 工具回调回复；这需要 Host 维护目录与 Provider 白名单，还要把持有者令牌交给 Agent。

Workspace Bot 已经具备所需的一切：按会话串行执行、`messageId` 去重、轮次与投递两套持久状态、权限请求自动拒绝，以及按权限分组和模式上限约束它启动的 Agent。外部 Channel 本就是“收入站消息、取待投递回复”的传输。中心可以作为另一种 Channel。

## 决策

1. **Channel 类型**：新增 `ChannelConfig::Hub`（`{"kind":"hub"}`），用现有 `bot.channel.create.request` 创建，默认停用，每个 Bot 至多一个（第二个返回 `BotConflict`）。daemon 不为它运行任何传输：中心通过自己的 Hub 会话投递输入、拉取回复。启用 Hub Channel 就是主人对中心的授权。和 ADR-226 一样，旧客户端会拒绝 `bot.channel.types.request` 中的未知值，因此不列入其中。
2. **发布**：daemon 在每条新 serve 链路上、以及可达 Bot 集合变化时调用 `hub.automation.publish`，正文为 version 2：
   ```json
   {
     "version": 2,
     "enabled": true,
     "bots": [{ "workspaceId": "wks_…", "channelId": "…", "title": "app", "state": "active" }]
   }
   ```
   `bots` 列出有已启用 Hub Channel 的未退役 Bot（最多 64 个），`title` 取 Workspace 标题或显示名，`state` 为 `active` 或 `paused`。`enabled` 表示其中至少一个 Bot 处于 `active`。中心没有声明这个方法时不发布。
3. **准入**：`open` 带 `service: {kind: "automation"}` 的会话经 `Admission::Automation` 进入本地 API，仅在至少一个 `active` Bot 有已启用的 Hub Channel 时接受。最后一个这样的 Channel 停用或删除、Bot 暂停或移除时，立即结束全部自动化会话。
4. **策略**（`crates/api/src/automation/policy.rs`，默认拒绝）只放行：

   | 方法                                       | 参数                                                                        | 结果                                                                                                                                  |
   | ------------------------------------------ | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
   | `bot.hub.message.send.request`（新）       | `workspaceId`、`channelId`、`peer`、`sender`、`thread`、`messageId`、`text` | `{turn, accepted, duplicate}`；全局积压已满时 `{"accepted":false}`，中心稍后重试                                                      |
   | `bot.turn.get.request`                     | `workspaceId`、`turnId`                                                     | `{turn}`                                                                                                                              |
   | `bot.turn.cancel.request`                  | `workspaceId`、`turnId`                                                     | 现有结果                                                                                                                              |
   | `bot.hub.delivery.ack.request`（新）       | `workspaceId`、`turnId`、`status: delivered\|uncertain`                     | `{}`；只对已完成的轮次，重复确认无副作用，已送达的不再改动                                                                            |
   | `bot.hub.conversation.close.request`（新） | `workspaceId`、`conversationId`                                             | `{}`；未知会话也返回 `{}`；有轮次在运行时 `BotConflict`；删除会话、它的回执和排队输入，并关闭隐藏 session，不再占用 4096 个会话的名额 |

   每个方法都必须落在该 Workspace 当前 Bot 的已启用 Hub Channel 上，轮次与会话也必须属于该 Channel，否则 `forbidden`。三个新方法由 Bot 自身检查 Channel；两个轮次方法先由策略调用 `Bots::check_hub_turn`。其他请求、事件、响应和二进制帧一律 `forbidden`。三个新方法也对主人会话开放，访客不可调用。

5. **会话划分**：Bot 按 `(channel, peer, sender, thread)` 定位会话。中心把自己的会话 ID 放在 `thread`，因此续接策略完全由中心决定；`peer`、`sender` 只用于展示。
6. **回复**：Hub Channel 的轮次完成后投递状态保持 `pending`。中心轮询 `bot.turn.get.request`，把 `messages`（[ADR-227](adr-227-bot-multi-message-output.md) 的多条可见回复）按顺序以空行连接后发到平台，旧 daemon 没有 `messages` 时用 `reply`；发出后用 `bot.hub.delivery.ack.request` 报告结果。

## 后果

- 中心能做的只有：向 Hub Channel 投递文本、读取和取消该 Channel 的轮次、确认投递、关闭会话。它不能创建 Agent，也不需要公网 MCP 端点或持有者令牌；Bot 用这些文本做什么由 Bot 的权限分组与模式上限决定（ADR-215）。
- 发布的 capability 从 234 增至 237。
- `model::bot::Store` 新增 `delete_conversation`：先删会话路由再删回执，中断时只留下不可达的回执文件。
- 只有最终答复，没有运行中的进度；同一会话只排队，不支持 steer。
- 客户端协议的 Channel 配置联合类型加入 `hub`，Bot 设置页能列出它；添加 Hub Channel 的界面留待后续，在此之前可通过 `bot.channel.create.request` 创建。

## 验证

- `cargo test -p bot`、`cargo test -p api`、`cargo test -p persistence`、`cargo test -p daemon --lib` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、`packages/protocol` 与 `apps/mobile` 的 `typecheck` 通过。

## Test coverage

未测量：本次未要求覆盖率统计。新增行为由定向测试覆盖：Hub Channel 的创建、发布、投递、确认与关闭（`bot::state::tests::hub`），准入与策略（`api::automation`、`api::automation::policy`），in-process 会话从准入到关闭的全过程（`connection::bridge::tests`），以及发布循环（`host::hub::automation::tests`）。
