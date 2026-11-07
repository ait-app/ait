# ADR-223：中心发起的 Hub 自动化会话

- 状态：已实现（daemon 侧；客户端设置界面待后续）
- 日期：2026-11-07
- 关系：部分推翻 [ADR-045](adr-045-remove-hub-chat-loop.md) 与 [ADR-137](../clients/adr-137-remove-retired-feature-protocol.md) 对 Hub 的移除：不恢复上游 Paseo 的 `hub.management.*` / `hub.execution.*` 方法，改为在 [ADR-180](adr-180-hub-serve-endpoint.md) 的 serve 链路上接受中心会话；沿用 [ADR-208](adr-208-workspace-share-guests.md) 访客会话的准入方式，以及 [ADR-145](../bot/adr-145-bot-permission-denial.md) 的权限自动拒绝。中心侧协议见 ait-server `docs/automation.md`。

## 背景

用户希望在飞书、Slack、Telegram 中 @机器人，由中心选择本账号的一台 Host 运行 Agent，并把结果回复到原会话。中心不能执行 Agent，只能把请求交给 Host。Host 必须能独立决定是否接受、在哪些目录、用哪些 Provider；旧 daemon 会忽略未知字段，把中心会话当作账号主人，所以准入不能只靠中心。

## 决策

1. **本地开关**：`<data_dir>/hub/automation.json` 保存 `model::automation::Settings`（`enabled`、`roots`、`providers`），默认关闭。只有账号主人通过 `automation.settings.get.request` / `automation.settings.set.request` 读写；访客会话与自动化会话都到不了这两个方法。
2. **发布**：daemon 在每条新 serve 链路和每次设置变化后调用 `hub.automation.publish`（`version: 1`）。中心没有声明这个方法时不发布。中心只向最近一次发布 `enabled: true` 的链路打开自动化会话。
3. **准入**：`open` 带 `service: {kind: "automation"}` 的会话，经 `relay::hub::bridge::Admission::Automation` 进入本地 API。开关关闭时拒绝，Hub 会话以 `forbidden` 结束；关闭开关会立即结束全部自动化会话。其他 `service` 类型，或与 `visitor`、下载同时出现时，以 `unsupported` 拒绝。
4. **策略**（`crates/api/src/automation/policy.rs`，默认拒绝）：
   - `agent.create.request`：只接受 `config`、`labels`、`idempotencyKey`（必填）。`config` 只能是 `provider`、`model`、`cwd`、`title`、`modeId`、`thinkingOptionId`、`mcpServers`。`cwd` 解析真实路径后必须位于某个 `roots` 之内，Provider 须在允许列表内，`mcpServers` 只能是名为 `ait_hub` 的 HTTP 服务；不能写 Bot 或访客标签；强制写入 `labels.origin = automation`。
   - `agent.message.send.request`（只允许文本，`messageId` 必填）、`agent.finish.wait.request`、`agent.cancel.request`、`agent.get.request`：目标 Agent 必须带 `origin = automation`，并固定为完整 ID。
   - 其他请求、事件、响应和二进制帧一律 `forbidden`。这限制的是 RPC，不是沙箱：Agent 自己的进程以 Host 用户身份运行。
5. **权限**：带 `origin = automation` 的 Agent 与 Bot 会话一样，执行期间的权限请求自动拒绝，也拒绝手动授权；它们仍是普通可见 Agent，用户可以在 Ait 中查看和继续对话。
6. **幂等**：沿用已有机制。`idempotencyKey` 使重试的创建不产生第二个 Agent；`messageId` 由 timeline 收件回执保证同一消息只投递一次，无法确认时返回 `agent_io`，不会重发。

## 后果

- 新增 2 个 owner 方法，发布的 capability 从 216 增至 218。
- 账号主人可以给自己的普通 Agent 打上 `origin = automation`，这只会让该 Agent 自动拒绝权限请求，并允许本账号的中心会话驱动它；中心只驱动自己创建并记录的 Agent。
- 客户端设置界面不在本次范围；在界面完成前，可通过 owner RPC 开启。

## 验证

- `cargo test -p model automation`、`cargo test -p relay --lib`、`cargo test -p api`、`cargo test -p provider --lib agent_manager`、`cargo test -p daemon` 通过。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all --check` 通过。

## Test coverage

未测量：本次未要求覆盖率统计。新增行为由定向测试覆盖：设置存储与准入（`automation::tests`）、请求策略（`automation::policy::tests`）、in-process 会话准入与关闭（`connection::bridge::tests`）、relay 的 `service` 解析（`hub::tests`）、权限自动拒绝（`agent_manager::tests::automation_permissions`）和发布循环（`host::hub::automation::tests`）。
