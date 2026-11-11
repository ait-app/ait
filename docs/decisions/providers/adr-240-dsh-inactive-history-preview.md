---
id: ADR-240
status: proposed
date: 2026-11-11
paths:
  - crates/provider/src/local/deepseek_harness
  - crates/provider/src/ports/native_history.rs
  - crates/provider/src/ports/agent_session.rs
  - crates/provider/src/protocol/native_sessions.rs
  - crates/provider/src/service/agent_manager/native_sessions.rs
  - crates/provider/src/service/provider_catalog.rs
  - crates/provider/src/service/agent_execution/operation.rs
  - crates/provider/src/rpc/agent_execution
  - crates/provider/src/dispatch
  - packages/protocol/src/messages.ts
  - packages/protocol/src/agent-types.ts
  - packages/client/src/daemon-client.ts
  - packages/client/src/protocol
  - apps/mobile/src/components/import-session-sheet.tsx
  - apps/mobile/src/components/session-preview-sheet.tsx
---

# ADR-240：DSH 非活动历史只读浏览与固定截面预览

- 关系：沿用 [ADR-133](adr-133-cached-timeline-first.md) 的持久 Timeline 优先与
  [ADR-125](adr-125-durable-timeline-projection-pages.md) 的投影分页；本决定只约束
  DSH 原生历史的**非激活读取**与新的只读预览接口，不改变导入、恢复或写入生命周期。

## 背景与证据

主线 DSH 适配器的历史读取（`native/history.rs`）与冷会话预览（`native/native_sessions/previews.rs`）
通过 `session/follow` 订阅取得快照。官方 DSH `0.2.2-rc.1`（npm 包 `@deepseek-ai/dsh`）的
`dsh-api-session-controller` 公共源码表明：Web 客户端的 follow 打开携带 `activate: true`，
控制器在 `request.activate === true` 且会话处于 prepared 冷状态时执行 promote，
把冷会话升级为活动 Agent。因此主线的“只读”浏览/导入预览可能激活原生写入者。

同一安装包的公共 Remote 端点声明（typert host 描述）确认存在非激活读取路径：

- `session/list`：读取全部可见会话行，不恢复 Agent；行含 `sessionId`、`cwd`、`origin`、
  `parentSessionId`、`updatedAt` 与投影缓存提示。
- `session/projections`：返回精确 `sequenced` 基线（`asOfSeq` + `values`）；
  仅当持久格式需要迁移时返回 `migration-required` 缓存提示。不激活会话。
- `session/page`：冷安全、按消息对齐的历史页，支持 `address`、固定 `throughSeq`、
  backward `beforeSeq` 与 `maxMessages`。

## 决定

1. **读取端点语义（旧 → 新）**：DSH 历史/导入/预览读取一律改用
   `session/list`（根身份与 cwd 校验）→ `session/projections`（固定权威截面）→
   `session/page`（固定 `throughSeq` 分页）。缺失权威投影接口或基线非法时直接失败，
   **不回退**到会激活会话的 follow。`session/follow` 仅保留给显式创建/恢复写入者的路径；
   恢复在 follow 之前先做只读根身份校验。原生 ID 映射全部在后端完成，前端不解析原生文本。
2. **只读预览接口（新增，无删除）**：新增类型化方法
   `provider.sessions.preview.get.request/response`（Operation `SessionPreview`，
   `Access::Discovery`，无 Agent 目标）。请求 `{providerId, providerHandleId, cwd, cursor?, limit?}`；
   响应 `{providerId, providerHandleId, cwd, title, lastActivityAt, active, entries[], olderCursor, hasOlder}`，
   `entries` 复用既有 `NativeItem` 内容（key/turnId/timestamp/item），在借用式 wire
   投影中省略缺失的 `turnId`，保留真实原生 ID 字符串；不改变持久 `NativeItem` 的 null
   表示，也不放宽客户端类型。不引入通用 `native_action` 网关或无消费者的 Provider API。
3. **固定截面与游标绑定**：首次预览取 `session/projections` 的 `asOfSeq` 为固定截面；
   后续页携带不透明游标（base64url 编码的 typed 字段：provider、nativeID、cwd、throughSeq、beforeSeq，
   可选 `window: {startSeq, skippedItems}`），每页都绑定同一 `address` 与 `throughSeq`。
   原生页投影超过展示上限时，游标保存精确记录窗口与已返回展示项数，重读同一窗口后继续，
   而不是截断展示项后跳过整页；同一原生事件产生多项、最后原生页仍有剩余项时也不会丢失。
   这是新预览功能内部游标格式的修正；公开 RPC/SDK 的 opaque string 契约不变，无服务端游标缓存。
   后端在原生读取前拒绝跨身份、跨目录与非法窗口边界，再按有界投影验证 offset；
   前端只原样回传游标，不解析 key 或原生 JSON。单次响应的全部读取（包括窗口重读）共用
   ≤16 原生页、≤8 MiB 记录预算，展示项 ≤100，响应另受既有 900 KiB RPC 预算约束。
   返回数量可小于请求上限；消费者以 `olderCursor`/`hasOlder` 判断结束，不能按页长推断结束。
4. **能力与旧主机降级**：方法存在性只作为**主机版本**门槛（客户端 feature
   `importSessionPreview`）；逐 Provider 支持由快照新增可选字段 `supportsSessionPreview`
   广告（当前仅 DSH 原生交互传输为 true，ACP 为 false）。旧主机无此方法时客户端不发送请求；
   旧客户端忽略新增可选字段，既有消息与导入行为不变。未广告能力的 Provider 在 UI 不显示预览入口。
5. **UI 行为**：导入会话列表行新增独立只读预览入口（Eye），与既有“点击行即导入”分离；
   预览面板按固定截面分页渲染 Timeline 投影，提供“加载更早”与活动轮次只读提示。
   UI 对全部 newest-first 页整体反转，保留稳定项 key；查询按 Host 客户端代次/连接 epoch 隔离，
   取消后不接受迟到结果。预览的点击/键盘路径不触发导入、恢复、prompt 或写入者激活。
6. **时间语义**：只读 wire 摘要不携带原生创建时间；导入契约允许以观察时间代替，
   UI/文档必须标注其为观察/导入时间，不得冒充原生创建时间。

## 读取允许清单（运行时审计）

非活动浏览/预览期间允许的原生请求仅限：`session/list`、`session/projections`、
`session/page`、`session/modelCatalog`（导入配置回退）、`session/attachment`（图片只读水合）。
禁止出现：`session/follow`、`session/create`、`session/prompt`、`session/cancel`、
`session/selectModel`、`commands/execute` 及任何 ACP `session/new`、`session/load`、chat 请求；
活动 Agent 状态不变。

## 验证

离线 fixture 覆盖：固定截面分页与游标绑定、历史增长后游标不漂移、伪造/跨scope 游标在原生读取前拒绝、
非法基线无激活回退、空历史负游标、未完成轮次只读观察且写入者可继续、ACP 拒绝。
浏览器端到端与已安装 DSH 的只读冒烟在 PR 描述中记录；不发送模型请求。
