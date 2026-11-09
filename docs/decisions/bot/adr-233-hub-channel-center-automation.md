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

- 状态说明：已实现（daemon 侧；客户端添加 Hub Channel 的界面待后续；Webhook Channel 含客户端）。2026-11-10 增加 Webhook Channel（第 7–9 条）
- 关系：扩展 [ADR-139](adr-139-workspace-bots-and-owned-channels.md) 的 Channel 传输与 [ADR-213](adr-213-single-workspace-manager-bot.md) 的 v2 接口；部分推翻 [ADR-045](../daemon/adr-045-remove-hub-chat-loop.md) 与 [ADR-137](../clients/adr-137-remove-retired-feature-protocol.md) 对 Hub 的移除：不恢复上游 Paseo 的 `hub.management.*` / `hub.execution.*`，改为在 [ADR-180](../daemon/adr-180-hub-serve-endpoint.md) 的 serve 链路上接受中心会话，准入方式沿用 [ADR-208](../daemon/adr-208-workspace-share-guests.md) 的访客会话。中心侧见 ait-server `docs/automation.md`。

## 背景

用户希望在飞书、Slack、Telegram 中 @机器人，由中心把消息交给本账号某台 Host 处理，并把结果回复到原会话。上一版设计让中心直接创建 Agent，并通过公网 MCP 工具回调回复；这需要 Host 维护目录与 Provider 白名单，还要把持有者令牌交给 Agent。

Workspace Bot 已经具备所需的一切：按会话串行执行、`messageId` 去重、轮次与投递两套持久状态、权限请求自动拒绝，以及按权限分组和模式上限约束它启动的 Agent。外部 Channel 本就是“收入站消息、取待投递回复”的传输。中心可以作为另一种 Channel。

## 决策

1. **Channel 类型**：新增 `ChannelConfig::Hub`（`{"kind":"hub"}`），用现有 `bot.channel.create.request` 创建，默认停用，每个 Bot 至多一个（第二个返回 `BotConflict`）。daemon 不为它运行任何传输：中心通过自己的 Hub 会话投递输入、拉取回复。启用 Hub Channel 就是主人对中心的授权。和 ADR-226 一样，旧客户端会拒绝 `bot.channel.types.request` 中的未知值，因此不列入其中。
2. **发布**：daemon 在每条新 serve 链路上、以及可达 Channel 集合变化时调用 `hub.automation.publish`，正文为 version 2：
   ```json
   {
     "version": 2,
     "enabled": true,
     "bots": [
       {
         "workspaceId": "wks_…",
         "channelId": "…",
         "title": "app",
         "state": "active",
         "kind": "hub"
       }
     ]
   }
   ```
   `bots` 每项是一个已启用的中心 Channel（Hub 或 Webhook，见第 7 条），Bot 未退役，最多 64 项；同一 Workspace 可出现多次。`title` 取 Workspace 标题或显示名，`state` 为 `active` 或 `paused`，`kind` 为 `hub` 或 `webhook`：中心的 Trigger 按 Workspace 找 `hub`，Webhook 消息按 `channelId` 找 `webhook`，缺省按 `hub`。`enabled` 表示其中至少一项处于 `active`。中心没有声明这个方法时不发布。
3. **准入**：`open` 带 `service: {kind: "automation"}` 的会话经 `Admission::Automation` 进入本地 API，仅在至少一个 `active` Bot 有已启用的中心 Channel 时接受。最后一个这样的 Channel 停用或删除、Bot 暂停或移除时，立即结束全部自动化会话。
4. **策略**（`crates/api/src/automation/policy.rs`，默认拒绝）只放行：

   | 方法                                       | 参数                                                                        | 结果                                                                                                                                  |
   | ------------------------------------------ | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
   | `bot.hub.message.send.request`（新）       | `workspaceId`、`channelId`、`peer`、`sender`、`thread`、`messageId`、`text` | `{turn, accepted, duplicate}`；全局积压已满时 `{"accepted":false}`，中心稍后重试                                                      |
   | `bot.turn.get.request`                     | `workspaceId`、`turnId`                                                     | `{turn}`                                                                                                                              |
   | `bot.turn.cancel.request`                  | `workspaceId`、`turnId`                                                     | 现有结果                                                                                                                              |
   | `bot.hub.delivery.ack.request`（新）       | `workspaceId`、`turnId`、`status: delivered\|uncertain`                     | `{}`；只对已完成的轮次，重复确认无副作用，已送达的不再改动                                                                            |
   | `bot.hub.conversation.close.request`（新） | `workspaceId`、`conversationId`                                             | `{}`；未知会话也返回 `{}`；有轮次在运行时 `BotConflict`；删除会话、它的回执和排队输入，并关闭隐藏 session，不再占用 4096 个会话的名额 |

   每个方法都必须落在该 Workspace 当前 Bot 的某个已启用中心 Channel 上，轮次与会话也必须属于这类 Channel，否则 `forbidden`。三个新方法由 Bot 自身检查 Channel；两个轮次方法先由策略调用 `Bots::check_hub_turn`。其他请求、事件、响应和二进制帧一律 `forbidden`。三个新方法也对主人会话开放，访客不可调用。

5. **会话划分**：Bot 按 `(channel, peer, sender, thread)` 定位会话。中心把自己的会话 ID 放在 `thread`，因此续接策略完全由中心决定；`peer`、`sender` 只用于展示。
6. **回复**：中心 Channel 的轮次完成后投递状态保持 `pending`。中心轮询 `bot.turn.get.request`，把 `messages`（[ADR-227](adr-227-bot-multi-message-output.md) 的多条可见回复）按顺序以空行连接后发到平台，旧 daemon 没有 `messages` 时用 `reply`；发出后用 `bot.hub.delivery.ack.request` 报告结果。
7. **Webhook Channel**：新增 `ChannelConfig::Webhook`（`{"kind":"webhook","endpointId":"…","url":"https://…"}`），由中心托管一个 HTTP 端点，调用经中心的 Hub 会话像 Hub Channel 一样投进 Bot。用户在 Bot 设置页一步创建，像接入微信一样，不需要打开中心网页或写 Trigger。每个 Bot 可以有多个，daemon 不运行传输。
   - 只能由 `bot.channel.webhook.create.request`（`{workspaceId}`）创建：daemon 先检查 Bot 和 Channel 上限，再在本机 serve 链路上调用中心的 `hub.automation.webhook.create`（`{workspaceId, channelId}` → `{endpointId, url, secret}`），然后保存已启用的 Channel，返回 `{state:"created", channel, secret}`。保存失败时尽力调用 `hub.automation.webhook.delete`。`bot.channel.create.request` 收到 `webhook` 配置返回 `InvalidMessage`，`bot.channel.update.request` 只能切换启用状态，改配置返回 `BotConflict`。
   - 密钥只返回这一次，daemon 不保存。`bot.channel.webhook.rotate.request`（`{workspaceId, channelId}`）对同一 Channel 再次调用中心的创建方法，中心保留端点并换新密钥，返回 `{state:"rotated", secret, url}`。
   - 中心不可用时两个方法都返回 `{state:"failed", reason}`，不报错：`centerOffline`（未接入中心、链路离线或 30 秒内无应答）、`centerUnsupported`（中心未声明方法）、`automationUnavailable`、`limitReached`、`centerError`。
   - 删除 Webhook Channel、移除 Bot 时，daemon 尽力调用 `hub.automation.webhook.delete`（`{channelId}`）。中心不可达时只在本地删除；中心只向 Host 已发布的 Channel 投递，残留端点不会再进 Bot。
8. **端点地址**：`url` 只接受 `https`，或回环地址上的 `http`（本地中心）。它会被复制到其他系统，因此不能是任意明文地址。
9. **客户端**：`bot.channel.types.request` 不列 `webhook`（同第 1 条）。客户端在两个新方法都可用时显示「Webhook」（`features.workspaceBotWebhookSetup`），创建后显示地址、一次性密钥和 curl 示例，均可复制；Channel 卡片显示地址和「重新生成密钥」。两个方法走文件操作通道，访客不可调用。

## 后果

- 中心能做的只有：向 Hub Channel 投递文本、读取和取消该 Channel 的轮次、确认投递、关闭会话。它不能创建 Agent，也不需要公网 MCP 端点或持有者令牌；Bot 用这些文本做什么由 Bot 的权限分组与模式上限决定（ADR-215）。
- 发布的 capability 从 234 增至 237，Webhook Channel 再增至 239。
- Webhook 调用方拿到 202 和状态地址，用同一密钥轮询结果；端点、密钥哈希、限流与结果查询都在中心（ait-server `docs/automation.md`）。
- 创建时如果中心已建好端点而应答丢失，daemon 尽力删除它；daemon 也离线时，中心会留下一个从未发布的端点，直到在中心删除。
- `model::bot::Store` 新增 `delete_conversation`：先删会话路由再删回执，中断时只留下不可达的回执文件。
- 只有最终答复，没有运行中的进度；同一会话只排队，不支持 steer。
- 客户端协议的 Channel 配置联合类型加入 `hub`，Bot 设置页能列出它；添加 Hub Channel 的界面留待后续，在此之前可通过 `bot.channel.create.request` 创建。

## 验证

- `cargo test -p bot`、`cargo test -p api`、`cargo test -p persistence`、`cargo test -p daemon --lib` 通过。
- Webhook Channel（2026-11-10）：`cargo test --workspace` 与 daemon 集成测试通过（daemon 的 `device::` 测试在并行下偶发失败，串行通过，与本次改动无关）；`packages/protocol`、`packages/client` 的 typecheck 与测试，`apps/mobile` 的 typecheck 与 `src/bots`、`src/i18n` 测试通过。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、`packages/protocol` 与 `apps/mobile` 的 `typecheck` 通过。

## Test coverage

未测量：本次未要求覆盖率统计。新增行为由定向测试覆盖：Hub 与 Webhook Channel 的创建、发布、投递、确认与关闭（`bot::state::tests::hub`），Webhook 的中心调用、失败原因与删除（`bot::channels::webhook::tests`），客户端创建与重新生成密钥（`apps/mobile/src/bots/bot-channels.test.tsx`），准入与策略（`api::automation`、`api::automation::policy`），in-process 会话从准入到关闭的全过程（`connection::bridge::tests`），以及发布循环（`host::hub::automation::tests`）。
