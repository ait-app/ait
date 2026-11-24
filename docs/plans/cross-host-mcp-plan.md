# 跨主机 MCP 与子 Agent：实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让一台主机上的 Agent 经 daemon MCP 发现同账号的其他主机，并在那里创建、提示、观察 Agent。

**Architecture:** 账号中心的 Hub 增加 `peer` feature：serve 链路可向兄弟主机开 `k:"peer"` 会话，中心只投递给发布了“接受”的主机。
daemon 在会话里转发 MCP 工具调用，接收方用自己的 MCP 层以“对端调用者”身份执行。客户端从两个标签把远程子 Agent 关联到父 Agent。

**Tech Stack:** Rust（`ait-server` 的 axum + 内存 Hub；本仓库的 `relay`、`model`、`api`、`daemon`），TypeScript（`packages/protocol`、`apps/mobile`），Markdown 文档。

**Spec:** [docs/plans/cross-host-mcp.md](cross-host-mcp.md)。每个任务的实现者都要读它；本计划只补充规格没有定下的名字、签名与数值。

## Global Constraints

- 三部分按 A → B → C 的顺序交付。A 在 `ait-server` 仓库（`/Users/necokeine/Documents/ait-server`）的独立分支上，先合并并部署；B、C 在本仓库。
- 改 Rust 之前阅读并遵守 [Rust 规范](../policy/rust.md)；改文件之前运行 `node scripts/adr.mjs governs <path>` 查看约束它的 ADR。
- 只运行改动代码及直接相关行为的测试；不运行整个工作区的测试或覆盖率。交付前在本仓库运行 `npm run verify`。
- 测试等待事件、消息或进程退出，不等待时间；不新增 `sleep`、timer promise 或 `waitForTimeout`。
- Markdown 与 TypeScript 用 `npx oxfmt <file>` 格式化，不用 prettier。
- 省略 `hostId` 的调用结果必须与改动前逐字节相同；不改旧工具名的别名（ADR-295）。
- 线上名字（全部任务共用，不得改写）：
  - Hub feature `peer`；会话类型 `k:"peer"`；转给目标的 `open` 头字段 `peer: {host_id, server_id, name}`。
  - 中心方法 `hub.peers.publish`（参数 `{"version":1,"enabled":bool}`，返回 `{"enabled":bool}`）与 `hub.peers.list`（参数 `{}`，
    返回 `{"hosts":[{host_id,node_id,server_id,name,hostname,platform,app_version,state,peer:{supported,enabled}}]}`）。
  - 关闭码 `unknown_target`、`forbidden`、`unsupported`、`peer_disabled`、`capacity`；发起方空闲关闭用 `done`。
  - 每条链路发起的与接受的 peer 会话各至多 8 个（`MAX_PEER_SESSIONS = 8`），不计入每链路 16 个会话。
  - 会话内消息（UTF-8 JSON 文本）：`open` 与 `accept` 的 payload 都是 `{"v":1}`；请求 `{"id":u64,"tool":string,"arguments":object,"caller"?:{"agentId":string}}`；
    回复 `{"id":u64,"result":object}` 或 `{"id":u64,"error":{"code":i32,"message":string}}`。
    接收方自己产生的两个错误码固定为：`-32001` 本机当前不接受对端调用（随后以 `peer_disabled` 关闭会话），`-32000` 回复超过消息上限；其余 `error` 是工具调用本身被拒绝（如 `-32602`）。
  - 配置键 `mcp.acceptPeers`（布尔，缺省 `false`）。标签 `ait.parent.server-id`、`ait.parent.agent-id`，保留前缀 `ait.parent.`。
  - MCP：工具 `host_list`；参数 `hostId`（daemon 的 `serverId`）；转发结果在 `structuredContent.hostId` 给出目标主机。
  - A 侧失败结果的 `structuredContent.code`：`hub_unavailable`、`host_unknown`、`host_offline`、`host_not_accepting`、`host_unsupported`、
    `capacity`、`outcome_unknown`、`caller_not_allowed`、`host_rejected`（对端以 `error` 拒绝了这次调用，文本为对端给出的消息）。
  - `model::peer::Error` 另有 `Rejected { code: i32, message: String }`：对端回复了 `error`（`-32001` 映射为 `NotAccepting`，`-32000` 映射为 `TooLarge`，其余为 `Rejected`）。
  - `Peers::call(node, request, timeout: std::time::Duration)`：时限由调用方给出、由适配器执行——在 `accept` 之前到期为 `Offline`（请求未发出），请求发出之后到期为 `OutcomeUnknown`。
  - 发起方打开 peer 会话时声明 1 MiB 的接收窗口（`credit::WINDOW`），使不超过消息上限的回复总能送达。
  - 可跨主机的 13 个工具：`agent_create`、`agent_prompt`、`agent_get`、`agent_activity_read`、`agent_list`、`agent_wait`、`agent_cancel`、
    `agent_archive`、`workspace_create`、`workspace_list`、`provider_list`、`provider_model_list`、`profile_list`。
- 主机列表缓存 5 秒；发起方的 peer 会话空闲 60 秒后关闭。

## Review Focus

规格隐含、但容易漏测的五种情况；每条都已在所属任务里加了测试：

1. **回复丢失的 `agent_create`。** 请求发出后会话结束：A 必须返回 `outcome_unknown` 而不是“未创建”，且不自动重放（任务 B3、B6）。
2. **开关刚关闭、发布尚未到达中心。** B 仍会收到已建立会话上的请求，必须在本地拒绝（任务 B5）。
3. **旧中心。** `hub_features` 不含 `peer` 时链路照常上线，`host_list` 只有本机（任务 B1、B6）。
4. **`hostId` 等于本机。** 走本地路径且结果里没有多出来的 `hostId`（任务 B6）。
5. **远程父 Agent 的 id 与本机某个 Agent 相同。** 客户端按 `(serverId, agentId)` 关联，不能把它归到本机同 id 的 Agent 下（任务 C3）。

---

## 文件结构

**`ait-server`**

| 文件                                                | 职责                                                                |
| --------------------------------------------------- | ------------------------------------------------------------------- |
| `src/hub/wire.rs`                                   | `Features.peer`、`Kind::Peer`、`Outbound::Open.peer`                |
| `src/hub/account.rs`                                | serve 发起的 `open` 的路由、拒绝码、单独计数、关闭                  |
| `src/hub/peers.rs`（新）                            | `hub.peers.publish`、`hub.peers.list`，仿照 `src/hub/automation.rs` |
| `src/hub/services/protocol.rs`                      | 把两个方法加入 `METHODS`                                            |
| `src/hub/account/peer_tests.rs`（新）               | 以上行为的测试，沿用 `account_feature_tests.rs` 的辅助函数          |
| `docs/hub-protocol.md`、`docs/hub-services.md`      | 协议与身份规则                                                      |
| `docs/decisions/adr-008-hub-peer-sessions.md`（新） | 决策                                                                |

**本仓库**

| 文件                                                                                         | 职责                                                            |
| -------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `crates/model/src/peer.rs`（新）                                                             | 端口 `Peers` 与类型 `Host`、`HostState`、`Identity`、`Error`    |
| `crates/relay/src/hub/wire.rs`                                                               | 可选 feature、`SessionKind::Peer`、新帧与字段                   |
| `crates/relay/src/hub/peer.rs`（新）                                                         | 会话内消息的编解码；发起方的会话表与未决请求                    |
| `crates/relay/src/hub/mod.rs`、`session.rs`、`bridge.rs`                                     | 接收与发起两个方向接入链路                                      |
| `crates/persistence/src/storage/daemon_config.rs`、`crates/metadata/src/daemon/protocol.rs`  | `mcp.acceptPeers`                                               |
| `bins/daemon/src/host/hub/peers.rs`（新）                                                    | 发布循环，仿照 `automation.rs`                                  |
| `crates/api/src/mcp/peer.rs`（新）                                                           | B 侧：对端调用者的规则与执行入口                                |
| `crates/api/src/mcp/hosts.rs`（新）                                                          | A 侧：`host_list`、`hostId` 路由与失败映射                      |
| `crates/api/src/mcp/catalog.json`、`catalog.rs`、`mcp.rs`、`scope.rs`、`tools.rs`            | 目录标记、Schema、调用入口、保留标签                            |
| `crates/api/src/connection/bridge.rs`、`crates/api/src/lib.rs`                               | `LocalApi::peer` 的实现；`Shared.peers`、`Api::peer_acceptance` |
| `crates/domain/src/bot/permissions.rs`                                                       | `host_list` 为 `HostOnly`                                       |
| `packages/protocol/src/agent-labels.ts`、`messages.ts`、`paseo-tool-presentation/catalog.ts` | 标签读取、配置字段、工具展示                                    |
| `apps/mobile/src/screens/settings/host-page.tsx`、`src/i18n/resources/*`                     | 设置开关                                                        |
| `apps/mobile/src/subagents/select.ts`、`src/task-board/model.ts`                             | 跨主机关联                                                      |
| `skills/ait-orchestration/`、`docs/`                                                         | Skill、手册、规格、ADR、CHANGELOG                               |

---

## Part A：账号中心（`ait-server` 仓库）

### Task A1：`peer` feature 与 serve 发起的会话

**Files:**

- Modify: `src/hub/wire.rs`（`FEATURES`、`Features`、`Kind`、`Outbound::Open`）
- Modify: `src/hub/account.rs`（`receive` 的 `Inbound::Open` 分支、`peer()`、会话计数）
- Test: `src/hub/account/peer_tests.rs`（新；在 `account.rs` 里以 `#[cfg(test)] mod peer_tests;` 引入）

**Interfaces:**

- Produces：`wire::Features { peer: bool, .. }`；`wire::Kind::Peer`（线上为 `"peer"`）；
  `Outbound::Open { peer: Option<PeerInfo>, .. }`，`pub struct PeerInfo { pub host_id: Uuid, pub server_id: Uuid, pub name: String }`；
  `account::MAX_PEER_SESSIONS: usize = 8`；`Link::accepts_peers(&self) -> bool`（本任务恒为 `false`，A2 接上发布状态；测试用 `#[cfg(test)]` 的设置函数）。
- 会话表不变：发起方记在 `Session.access`，目标记在 `Session.serve`。`peer()` 现在按端点类型判断谁是哪一方，两端都是 serve 时改按链路 id 判断。

- [ ] **Step 1：写失败的测试**（沿用 `account_feature_tests.rs` 的 `with`、`open`、`accept`、`msg`、`keys`）

```rust
#[test]
fn a_serve_link_opens_a_peer_session_to_an_accepting_sibling() {
    // 两条 main serve 链路 a、b 都声明 peer，b 接受。
    // a 发 open{k:peer,to:{node:b}} → b 收到的头含 k:"peer"、from:a 的 node、peer:{host_id,server_id,name}。
    // b 发 accept → a 收到 accept；双向 msg 各自到达对方。
}

#[test]
fn peer_opens_are_refused_with_the_documented_codes() {
    // 目标不存在 → unknown_target；目标是自己的节点 → forbidden；目标未声明 peer → unsupported；
    // 目标未接受 → peer_disabled；发起方已有 8 个 peer 会话 → capacity；目标已接受 8 个 → capacity。
}

#[test]
fn peer_sessions_have_their_own_allowance() {
    // b 上已有 16 个所有者会话时仍可接受 peer 会话；8 个 peer 会话不减少所有者的 16 个名额。
}

#[test]
fn only_declared_main_serve_links_may_open_peer_sessions() {
    // access 链路发 k:peer → Fatal("forbidden")；未声明 peer 的 serve 链路发 k:peer → Fatal("invalid_frame")；
    // bulk 链路发 k:peer → Fatal("forbidden")。
}

#[test]
fn links_without_peer_never_see_the_new_field() {
    // 未声明 peer 的 serve 链路收到的普通 open 头的键集合与改动前相同（没有 "peer"）。
}
```

- [ ] **Step 2：运行并确认失败**：`cargo test hub::account::peer_tests` → 编译失败（`Kind::Peer` 不存在）。
- [ ] **Step 3：实现。** `Inbound::Open` 分支：`k == Some(Kind::Peer)` 时走独立函数 `open_peer(id, s, to, payload, window)`，
      不经过 `ep != Ep::Access` 的检查；其余路径不变。拒绝顺序与规格第 1 节一致。`PeerInfo` 取自发起方链路的 node 记录。
- [ ] **Step 4：运行并确认通过**：`cargo test hub::account` → 全部通过（含原有的 feature 测试）。
- [ ] **Step 5：提交**：`git commit -m "feat(hub): peer sessions between sibling serve links"`

### Task A2：`hub.peers.publish` 与 `hub.peers.list`

**Files:**

- Create: `src/hub/peers.rs`
- Modify: `src/hub/services/protocol.rs`（`METHODS`）、`src/hub/account.rs`（`Link.peers: bool`、`publish_peers`、关闭会话）、`src/hub/mod.rs`（分发）
- Test: `src/hub/account/peer_tests.rs`，以及 `src/hub/tests.rs` 里一条经 `call` 帧的端到端测试

**Interfaces:**

- Consumes：A1 的 `Link::accepts_peers`、`MAX_PEER_SESSIONS`。
- Produces：`Account::publish_peers(&mut self, link: Uuid, session: Uuid, enabled: bool) -> Result<(), Fatal>`；
  `Hub::peers_publish(&self, call: &Call) -> Result<Value>`、`Hub::peers_list(&self, call: &Call) -> Result<Value>`。
  两个方法只接受声明了 `peer` 的 main serve 链路，其余返回 403；`hub.peers.list` 的 Host 对象复用 `src/hub/account/hosts.rs` 生成 `hosts` 帧的函数，
  再加 `peer: {supported, enabled}`。

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn disabling_peers_closes_the_sessions_a_host_accepted() {
    // b 接受后与 a 建立 peer 会话；b 发布 enabled:false → 双方收到 close{code:"peer_disabled"}，b 发起的会话不受影响。
}

#[test]
fn a_fresh_link_starts_without_accepting_peers() {
    // 同一节点的新链路替换旧链路后，未重新发布前 open 得到 peer_disabled。
}

#[tokio::test]
async fn peers_list_reports_every_host_with_its_peer_state() {
    // 账号下三个 Host：在线且接受、在线未声明 peer、离线。
    // list 返回三项，peer 分别为 {supported:true,enabled:true}、{supported:false,enabled:false}、{supported:false,enabled:false}，
    // state 分别为 online、online、offline；access 链路调用 → 403。
}
```

- [ ] **Step 2：运行并确认失败**：`cargo test hub::` → 新测试失败。
- [ ] **Step 3：实现**，结构与 `src/hub/automation.rs` 的 `automation_publish` 相同；`version` 不是 1 时返回 400。
- [ ] **Step 4：运行并确认通过**：`cargo test hub::`。
- [ ] **Step 5：提交**：`git commit -m "feat(hub): peers publish and list center methods"`

### Task A3：协议文档与 ADR

**Files:**

- Modify: `docs/hub-protocol.md`（第 1 节方向规则、第 4 节帧表、第 10 节配额表、第 11 节 feature 表，新增 11.8 `peer`）
- Modify: `docs/hub-services.md`（“身份与方向”的两条规则、方法表）
- Create: `docs/decisions/adr-008-hub-peer-sessions.md`

- [ ] **Step 1：** 按规格第 1 节与本计划的“线上名字”改写正文；`hub-services.md` 明确写出放宽的两条规则及放宽的范围（只有 Host 元数据）。
- [ ] **Step 2：** ADR 写背景、决策（含“为什么不放开机器凭据的 access”）、后果。
- [ ] **Step 3：** 运行该仓库的文档检查（见其 `README.md`）并提交：`git commit -m "docs(hub): peer sessions"`。

---

## Part B：daemon（本仓库）

### Task B1：端口类型与 Hub 帧

**Files:**

- Create: `crates/model/src/peer.rs`；Modify: `crates/model/src/lib.rs`（`pub mod peer;`）
- Modify: `crates/relay/src/hub/wire.rs`、`crates/relay/src/hub/mod.rs`（`Grant` → `offer`）
- Test: `crates/relay/src/hub/wire/tests.rs`、`crates/relay/src/hub/tests/features.rs`

**Interfaces:**

- Produces（`model::peer`）：

```rust
pub enum HostState { Online, Reconnecting, Offline }
pub struct Host {
    pub server_id: Uuid, pub host_id: Uuid, pub node_id: Uuid,
    pub name: String, pub hostname: String, pub platform: String, pub app_version: String,
    pub state: HostState, pub supported: bool, pub enabled: bool,
}
/// 中心填写的发起方身份。
pub struct Identity { pub host_id: Uuid, pub server_id: Uuid, pub name: String }
pub enum Error { Unavailable, UnknownHost, Offline, NotAccepting, Unsupported, Capacity, TooLarge, OutcomeUnknown }
#[async_trait::async_trait]
pub trait Peers: std::fmt::Debug + Send + Sync {
    /// 本账号的全部 Host；没有链路或中心不支持时为 `Error::Unavailable`。
    async fn hosts(&self) -> Result<Vec<Host>, Error>;
    /// 向 `node` 发一次工具调用，返回对端的 `result` 对象。
    async fn call(&self, node: Uuid, request: Request) -> Result<serde_json::Value, Error>;
}
pub struct Request { pub tool: String, pub arguments: serde_json::Value, pub caller: Option<String> }
```

- Produces（`relay::hub::wire`）：`pub(super) const PEER: &str = "peer";`；`offer(bulk_lane: bool, peer: bool)`（`peer` 为 `grant.hub_features` 是否含它，
  不加入 `FEATURES`）；`SessionKind::Peer`；`Inbound::Open { peer: Option<model::peer::Identity>, .. }`；
  `Inbound::Accept { seq, s, w: Option<u32> }`；`Inbound::Close { seq, s, code: Option<String> }`；`Outbound::Open { seq, s, to: Target, k: &'static str, w: Option<u32> }`，
  其中 `pub(super) struct Target { pub(super) node: Uuid }`。

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn peer_is_offered_only_when_the_center_declares_it() {
    assert!(!offer(true, false).contains(&"peer"));
    assert!(offer(true, true).contains(&"peer"));
}

#[tokio::test]
async fn a_center_without_peer_still_gets_a_link() {
    // hub_features 不含 "peer"：start 成功，hello.f 不含 "peer"，链路 online。
}

#[test]
fn peer_frames_round_trip() {
    // decode open{k:"peer",peer:{..}} → SessionKind::Peer 与 Identity；decode accept；decode close{code:"peer_disabled"} → Some(code)；
    // encode Outbound::Open 的头为 {"t":"open","seq":..,"s":..,"to":{"node":..},"k":"peer"}。
}
```

- [ ] **Step 2：** `cargo test -p relay hub::wire hub::tests::features` → 失败。
- [ ] **Step 3：** 实现。此任务里 `SessionKind::Peer` 的 `open` 仍以 `unsupported` 拒绝，B2 接上。
- [ ] **Step 4：** `cargo test -p relay hub::` → 通过；`cargo test -p model` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(relay): peer feature and frames"`

### Task B2：接收对端会话

**Files:**

- Create: `crates/relay/src/hub/peer.rs`
- Modify: `crates/relay/src/hub/bridge.rs`（`LocalApi`）、`crates/relay/src/hub/session.rs`、`crates/relay/src/hub/mod.rs`（`Sessions::open`）、`crates/relay/src/test_support.rs`（`FakeApi`）
- Test: `crates/relay/src/hub/tests/peer.rs`（新）

**Interfaces:**

- Consumes：B1 的 `SessionKind::Peer`、`Identity`、`model::peer::Request`。
- Produces：

```rust
// bridge.rs，带默认实现，现有实现者不必改动
async fn peer(&self, from: &model::peer::Identity, request: model::peer::Request)
    -> Result<serde_json::Value, PeerRefusal>;   // 默认 Err(PeerRefusal::Disabled)
pub enum PeerRefusal { Disabled, Failed { code: i32, message: String } }

// peer.rs
pub(super) const VERSION: u64 = 1;
pub(super) fn decode_request(text: &[u8]) -> Result<(u64, model::peer::Request), Malformed>;
pub(super) fn encode_reply(id: u64, outcome: Result<serde_json::Value, (i32, String)>) -> Bytes;
```

- 接收的 peer 会话单独计数（至多 8 个），不占 `MAX_SESSIONS`。`open` 没有 `peer` 字段、payload 不是 `{"v":1}`、带 `visitor` 或 `service` 时以 `unsupported` 拒绝。
  每个请求在自己的任务里执行，回复顺序不保证；`PeerRefusal::Disabled` 时回复错误并以 `peer_disabled` 关闭会话。超过 1 MiB 的回复改为 `error{code:-32000}`。

- [ ] **Step 1：写失败的测试**（用 `tests.rs` 的脚本化中心）

```rust
#[tokio::test]
async fn peer_requests_reach_the_api_with_the_centers_identity() {
    // 中心发 open{k:peer,peer:{host_id,server_id,name}} + {"v":1} → daemon 回 accept + {"v":1}；
    // 发两条请求（id 1、2）→ FakeApi 记录到相同的 Identity 与各自的 tool/arguments/caller；两条回复的 id 与 result 对应。
}

#[tokio::test]
async fn malformed_peer_opens_and_refused_calls_close_the_session() {
    // 无 peer 字段 → close unsupported；API 返回 Disabled → 回复 error 后 close peer_disabled；
    // 第 9 个 peer 会话 → close capacity，同时所有者会话仍可打开。
}
```

- [ ] **Step 2：** `cargo test -p relay hub::tests::peer` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** `cargo test -p relay hub::` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(relay): serve peer sessions"`

### Task B3：发起对端调用与主机列表

**Files:**

- Modify: `crates/relay/src/hub/peer.rs`、`crates/relay/src/hub/mod.rs`（链路循环处理 `Accept`、发起会话上的 `Msg`、`Close`、`Credit`；`impl Peers for Connector`）
- Test: `crates/relay/src/hub/tests/peer.rs`

**Interfaces:**

- Consumes：B1 的 `Peers`、`Host`、`Error`、`Outbound::Open`；`Connector::call_center`。
- Produces：`impl model::peer::Peers for relay::hub::Connector`。
  - `hosts()`：`link.supports("hub.peers.list")` 为假 → `Error::Unavailable`；否则调用并解析。结果缓存 5 秒，逻辑链路变化时清空。
  - `call(node, request)`：每个目标节点至多一个会话，按需 `open`，请求 id 在会话内递增；会话空闲 60 秒后以 `done` 关闭（用 `tokio::time` 的可暂停时钟测试）。
  - 关闭码到错误：`unknown_target` → `Offline`，`unsupported` → `Unsupported`，`peer_disabled` → `NotAccepting`，`capacity` → `Capacity`，
    `forbidden` 与其余 → `Unavailable`。这些只适用于**请求尚未发出**的情况；请求已发出而会话结束（任何原因，包括逻辑链路丢失）→ `OutcomeUnknown`。
  - 物理断线后续传期间，未决请求保留，不报错。
  - 发起的 peer 会话同样单独计数（至多 8 个），不占 `MAX_SESSIONS`；超出时不发 `open`，直接返回 `Capacity`。

- [ ] **Step 1：写失败的测试**

```rust
#[tokio::test]
async fn a_call_opens_one_session_per_host_and_matches_replies_by_id() {
    // 对同一 node 并发两次 call → 中心只看到一个 open{k:"peer"}；按相反顺序回复 → 两个 future 各得到自己的 result。
}

#[tokio::test]
async fn refusals_before_the_request_map_to_their_errors() {
    // 中心对 open 回 close{code}：unknown_target/unsupported/peer_disabled/capacity → Offline/Unsupported/NotAccepting/Capacity。
}

#[tokio::test]
async fn a_session_that_ends_after_the_request_reports_an_unknown_outcome() {
    // accept 后请求已被中心收到，中心回 close{code:"peer_gone"} → Err(OutcomeUnknown)；随后的 call 重新 open。
}

#[tokio::test]
async fn calls_survive_a_resumed_link() {
    // 请求发出后断开物理连接并续传，中心随后回复 → call 得到 result。
}

#[tokio::test(start_paused = true)]
async fn an_idle_peer_session_is_closed_by_the_opener() {
    // 回复后推进 60 秒 → 中心收到 close{code:"done"}。            // virtual time
}

#[tokio::test]
async fn two_links_complete_a_call_through_a_forwarding_center() {
    // 两个 Connector 各连一个监听器，测试按 A1 的规则转发帧；A.call(B) 的结果等于 B 的 FakeApi 返回值，
    // B 看到的 Identity 是测试中心填写的那一个。
}

#[tokio::test]
async fn hosts_come_from_the_center_and_are_unavailable_without_the_method() {
    // welcome.center_methods 含 hub.peers.list → 解析三项 Host；不含 → Err(Unavailable)，且没有发出 call 帧。
}
```

- [ ] **Step 2：** `cargo test -p relay hub::tests::peer` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** `cargo test -p relay hub::` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(relay): call sibling hosts over peer sessions"`

### Task B4：`mcp.acceptPeers` 与发布

**Files:**

- Modify: `crates/persistence/src/storage/daemon_config.rs`（`validate_config`、默认配置）、`crates/metadata/src/daemon/protocol.rs`（校验补丁）
- Modify: `crates/api/src/lib.rs`（`Api::peer_acceptance`）
- Create: `bins/daemon/src/host/hub/peers.rs`；Modify: `bins/daemon/src/host/hub.rs`（`start` 里启动任务）
- Test: 各文件旁的测试；`bins/daemon/src/host/hub/peers/tests.rs`（新，仿照 `automation/tests.rs`）

**Interfaces:**

- Produces：`Api::peer_acceptance(&self) -> tokio::sync::watch::Receiver<bool>`，值为 `mcp.enabled && mcp.acceptPeers`，启动时读一次配置，
  之后随 `daemon.config.set.request` 的成功提交更新。`peers::run(center: Arc<dyn Center>, accepting: watch::Receiver<bool>, cancel: CancellationToken)`。
- 已有配置文件缺少 `acceptPeers` 时读作 `false`，写回时不强行补上；新配置的默认值显式为 `false`。

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn accept_peers_defaults_off_and_must_be_boolean() {
    // 默认配置 mcp.acceptPeers == false；缺少该键的旧配置能加载；非布尔值 → Invalid。
}

#[tokio::test]
async fn publication_follows_the_link_and_the_switch() {
    // 新逻辑链路上线 → call hub.peers.publish {"version":1,"enabled":false}；开关变为 true → 再发布 enabled:true；
    // 中心不支持该方法 → 不发出调用；同一链路上值未变 → 不重复发布。
}
```

- [ ] **Step 2：** `cargo test -p persistence daemon_config && cargo test -p metadata daemon::protocol && cargo test -p daemon host::hub::peers` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** 同一命令 → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(daemon): publish peer acceptance"`

### Task B5：对端调用者（接收方）

**Files:**

- Create: `crates/api/src/mcp/peer.rs`
- Modify: `crates/api/src/mcp/catalog.json`（13 个条目加 `"peer": true`）、`catalog.rs`（`Tool.peer: bool`，`#[serde(default)]`）、
  `mcp.rs`（把 `call` 中“授权之后”的部分抽成可复用的执行函数）、`scope.rs`（`reserved_labels` 的调用处增加 `ait.parent.`）、
  `crates/api/src/connection/bridge.rs`（`LocalApi::peer`）
- Test: `crates/api/src/tests/mcp/peer.rs`（新）、`crates/api/src/mcp/catalog/tests.rs`

**Interfaces:**

- Consumes：B2 的 `LocalApi::peer`、`PeerRefusal`；B1 的 `Identity`、`Request`。
- Produces：`pub(crate) async fn mcp::peer::serve(state: &Arc<Shared>, from: &Identity, request: Request) -> Result<Value, PeerRefusal>`，
  返回与本地 `tools/call` 相同的结果对象。常量 `PARENT_SERVER_LABEL = "ait.parent.server-id"`、`PARENT_AGENT_LABEL = "ait.parent.agent-id"`、
  `PARENT_LABEL_PREFIX = "ait.parent."`。
- 规则（规格第 4、5 节），按此顺序判定：
  1. `mcp.enabled` 或 `mcp.acceptPeers` 为 `false` → `PeerRefusal::Disabled`。
  2. 工具不存在、没有 `peer` 标记或其能力未安装 → `Failed{-32602,"Unknown or unavailable tool"}`。
  3. 参数含 `hostId` → `Failed{-32602,..}`；随后按主机凭据调用者看到的 Schema（不含 `hostId`）校验。
  4. `agent_create` 没有 `workspaceId` 或带 `cwd`、`workspace_create` 没有来源 → `isError` 结果，文本说明缺什么。
  5. `labels` 含 `ait.` 或 `paseo.` 前缀的键 → `isError` 的 `Forbidden` 结果。
  6. `agent_create` 且 `request.caller` 存在时，写入两个父标签（`from.server_id` 与 `caller`），不传 `callerAgentId`。
  7. 以无作用域、无调用者的方式执行，占用 `state.connections` 的额度并受 `CALL_TIMEOUT` 约束。
- 本机主机凭据的 `agent_create` 与 `agent_update` 同样拒绝 `ait.parent.` 前缀的标签。

- [ ] **Step 1：写失败的测试**

```rust
#[test]
fn exactly_the_thirteen_tools_are_marked_for_peers() {
    // 目录中 peer == true 的名字集合等于 Global Constraints 的 13 个。
}

#[tokio::test]
async fn a_peer_creates_an_agent_with_its_remote_parent_labels() {
    // acceptPeers 开；serve(agent_create{workspaceId,provider,title,initialPrompt}, caller:"a-1") → 结果含 agentId；
    // 该 Agent 的标签有 ait.parent.server-id == from.server_id、ait.parent.agent-id == "a-1"，没有 paseo.parent-agent-id。
}

#[tokio::test]
async fn peers_are_confined_to_their_rules() {
    // terminal_create、host_list → Unknown or unavailable tool；参数带 hostId → -32602；
    // agent_create 无 workspaceId → isError；agent_create 带 cwd → isError；labels{"ait.x":"1"}、{"paseo.parent-agent-id":"x"} → Forbidden。
}

#[tokio::test]
async fn a_host_that_stopped_accepting_refuses_established_peers() {
    // acceptPeers 关或 mcp.enabled 关 → Err(PeerRefusal::Disabled)，没有创建任何 Agent。
}

#[tokio::test]
async fn nobody_else_writes_the_parent_labels() {
    // 主机凭据的 tools/call agent_create 与 agent_update 带 labels{"ait.parent.agent-id":"x"} → isError Forbidden。
}
```

- [ ] **Step 2：** `cargo test -p api mcp::` → 失败。
- [ ] **Step 3：** 实现。授权判定仍只有一处：`serve` 自己完成第 1–6 步后调用抽出的执行函数，不在 `scope.rs` 的 Bot 规则里加分支。
- [ ] **Step 4：** `cargo test -p api mcp` → 通过（含原有 MCP 测试）。
- [ ] **Step 5：** 提交：`git commit -m "feat(api): serve MCP calls from sibling hosts"`

### Task B6：`host_list` 与 `hostId`（发起方）

**Files:**

- Create: `crates/api/src/mcp/hosts.rs`
- Modify: `crates/api/src/mcp/catalog.json`（新增 `host_list`，`methods: []`，`readOnly: true`）、`catalog.rs`（`schema` 对主机凭据调用者给带 `peer` 标记的工具加
  `hostId: {"type":"string","minLength":1}`）、`mcp.rs`（`call` 在取得调用者之后、执行之前路由；`initialize` 的 `instructions` 加一句）、
  `crates/api/src/lib.rs`（`Shared.peers: Arc<dyn Peers>`，默认是 `relay.hub()` 的克隆；测试用构造函数可替换）、
  `crates/domain/src/bot/permissions.rs`（`("host_list", HostOnly)`）
- Test: `crates/api/src/tests/mcp/hosts.rs`（新）、`crates/domain/src/bot/permissions/tests.rs`

**Interfaces:**

- Consumes：B1 的 `Peers`、`Host`、`Error`、`Request`；B5 的 `Tool.peer`。
- Produces：
  - `host_list` 结果：`{"accepting":bool,"hosts":[{"hostId","name","hostname","platform","appVersion","state","self","available","reason"?}],"note"?}`。
    本机总在首位；`Peers::hosts` 失败时只有本机并带 `note`。`available` 为 `state == online && supported && enabled`，
    `reason` 依次取 `offline`、`unsupported`、`not_accepting`。本机的 `available` 恒为 `true`。
  - 路由：`hostId` 缺省或等于本机 `serverId` → 去掉参数走原路径，结果不加 `hostId`。否则：
    作用域调用者不会走到这里（Schema 没有 `hostId`）；调用者 Agent 在沙箱化 Worktree 或带 `ait.guest.share-id` → `caller_not_allowed`；
    主机不在列表 → `host_unknown`；`!available` → 对应的 `host_*` 码，不发起调用；否则 `Peers::call`，成功时把对端结果原样返回并在
    `structuredContent` 写入 `hostId`。
  - 错误映射：`Unavailable` → `hub_unavailable`，`UnknownHost` → `host_unknown`，`Offline` → `host_offline`，`NotAccepting` → `host_not_accepting`，
    `Unsupported` → `host_unsupported`，`Capacity` 与 `TooLarge` → `capacity`，`OutcomeUnknown` → `outcome_unknown`。
    全部是 `isError: true` 的工具结果，`structuredContent` 为 `{"hostId","code","error"}`；`outcome_unknown` 的文本固定为
    `The call may have run on the other host. Check with agent_list or agent_get before retrying.`
  - `initialize` 的 `instructions` 追加：`Use host_list to see other hosts on this account; pass hostId to run agent, workspace and provider tools there.`

- [ ] **Step 1：写失败的测试**（用实现 `Peers` 的假对象，记录收到的 `Request`）

```rust
#[tokio::test]
async fn host_list_puts_this_host_first_and_explains_unavailable_ones() {
    // 假 Peers 返回本机 + 在线接受 + 在线未接受 + 离线 + 在线不支持 → self/available/reason 依次为
    // (true,true,-)、(false,true,-)、(false,false,"not_accepting")、(false,false,"offline")、(false,false,"unsupported")。
}

#[tokio::test]
async fn without_a_hub_link_only_this_host_is_listed() {
    // 假 Peers::hosts → Err(Unavailable)：hosts 只有本机，note 非空，isError 为假。
}

#[tokio::test]
async fn a_remote_call_is_forwarded_without_host_id_and_tagged_on_return() {
    // tools/call agent_get{agentId:"x",hostId:<B>}，X-Ait-Agent-Id 指向本机 Agent "a-1" →
    // 假 Peers 收到 node == B 的 node、tool "agent_get"、arguments {"agentId":"x"}、caller Some("a-1")；
    // 返回结果的 structuredContent.hostId == <B>，其余字段与假对象给出的相同。
}

#[tokio::test]
async fn the_local_host_id_and_no_host_id_take_the_same_path() {
    // hostId == 本机 serverId 的 agent_list 结果与不带 hostId 的结果相等，且没有 hostId 键；假 Peers 没有收到 call。
}

#[tokio::test]
async fn failures_name_the_host_and_never_replay() {
    // 假 Peers::call → Err(OutcomeUnknown)：isError、code "outcome_unknown"、文本为固定句子，call 只被调用一次；
    // 未知 hostId → host_unknown；available 为假的主机 → host_not_accepting，call 未被调用。
}

#[tokio::test]
async fn scoped_sandboxed_and_guest_callers_cannot_route() {
    // Bot 会话与子树凭据：tools/list 没有 host_list，agent_get 的 Schema 没有 hostId，带 hostId 调用 → -32602；
    // 调用者 Agent 在 sandboxed-worktree Workspace、或带 ait.guest.share-id → code "caller_not_allowed"。
}

#[tokio::test]
async fn tools_without_the_peer_mark_refuse_host_id() {
    // terminal_list{hostId} → -32602（Schema 没有该属性）。
}
```

- [ ] **Step 2：** `cargo test -p api mcp:: && cargo test -p domain bot::permissions` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** `cargo test -p api mcp && cargo test -p domain bot::permissions && cargo test -p daemon --test process mcp` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(api): host_list and hostId routing"`

### Task B7：ADR 与服务端文档

**Files:**

- Create: `docs/decisions/daemon/adr-<编号>-cross-host-mcp-peer-sessions.md`（编号：扫描全部本地与远程分支的 `docs/decisions` 后取未占用的最小值；写计划时最大为 304）
- Modify: `docs/operations/mcp.md`（工具表加 `host` 域，新增“跨主机调用”一节）、`docs/architecture/connection.md`、`docs/architecture/README.md`、
  `docs/operations/daemon.md`（`mcp.acceptPeers`）、`CHANGELOG.md`、`docs/decisions/README.md`（`node scripts/adr.mjs index` 生成）

- [ ] **Step 1：** ADR front matter：`amends: [daemon/adr-148-daemon-mcp.md, clients/adr-218-hub-feature-negotiation.md]`，
      `paths: [crates/api/src/mcp, crates/api/src/mcp.rs, crates/relay/src/hub, crates/model/src/peer.rs, bins/daemon/src/host/hub]`。
      正文记录两条边界：peer 会话（跨组件契约）与对端调用者（权限边界，含 2026-11-24 确认的权限上限及未采用的固定上限）。
- [ ] **Step 2：** 手册写清：开关的含义（打开即信任本账号全部主机）、`host_list` 的字段、13 个工具、失败码、沙箱与访客限制只是尽力而为。
- [ ] **Step 3：** `node scripts/adr.mjs index && npm run check:docs && npx oxfmt <每个改动的文件>` → 无错误。
- [ ] **Step 4：** 提交：`git commit -m "docs: cross-host MCP"`

---

## Part C：客户端与 Skill（本仓库）

### Task C1：协议包

**Files:**

- Modify: `packages/protocol/src/agent-labels.ts`、`packages/protocol/src/messages.ts`（daemon 配置的两处 `mcp` schema 加可选的 `acceptPeers`）、
  `packages/protocol/src/paseo-tool-presentation/catalog.ts`
- Test: `packages/protocol/src/agent-labels.test.ts`（新）、`packages/protocol/src/paseo-tool-presentation/index.test.ts`

**Interfaces:**

- Produces：

```ts
export const PARENT_SERVER_ID_LABEL = "ait.parent.server-id";
export const REMOTE_PARENT_AGENT_ID_LABEL = "ait.parent.agent-id";
export function getRemoteParentFromLabels(
  labels: Record<string, unknown> | null | undefined,
): { serverId: string; agentId: string } | null; // 两个标签都是非空字符串时才返回
```

- 工具展示：`host_list` 的标签为 `List hosts`；带 `hostId` 的调用在摘要里显示主机（沿用目录里已有的摘要写法）。

- [ ] **Step 1：写失败的测试**

```ts
it("reads a remote parent only when both labels are present", () => {
  expect(
    getRemoteParentFromLabels({ "ait.parent.server-id": "s", "ait.parent.agent-id": "a" }),
  ).toEqual({
    serverId: "s",
    agentId: "a",
  });
  expect(getRemoteParentFromLabels({ "ait.parent.agent-id": "a" })).toBeNull();
  expect(getRemoteParentFromLabels({ "paseo.parent-agent-id": "a" })).toBeNull();
});

it("accepts a daemon config with and without mcp.acceptPeers", () => {
  /* 两种配置都能通过 schema；补丁 { mcp: { acceptPeers: true } } 通过 */
});

it("presents host_list and names the host of a routed call", () => {
  /* host_list → "List hosts"；agent_get{hostId:"h"} 的摘要含 "h" */
});
```

- [ ] **Step 2：** `npm run test --workspace=@ait/protocol -- src/agent-labels.test.ts src/paseo-tool-presentation/index.test.ts` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** 同一命令加 `npm run typecheck --workspace=@ait/protocol` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(protocol): remote parent labels and host tools"`

### Task C2：设置开关

**Files:**

- Modify: `apps/mobile/src/screens/settings/host-page.tsx`（在 `InjectPaseoToolsCard` 旁新增 `AcceptPeersCard`，结构相同）、
  `apps/mobile/src/i18n/resources/*`（每种语言）
- Test: `apps/mobile/src/screens/settings/` 下与 `InjectPaseoToolsCard` 相同位置的测试；`apps/mobile/src/i18n/resources.test.ts`

**Interfaces:**

- Consumes：C1 的配置字段。
- Produces：`testID="host-page-accept-peers-card"`；开关值为 `config?.mcp.acceptPeers === true`；`patchConfig({ mcp: { acceptPeers: next } })`。
  文案键 `settings.host.orchestration.acceptPeers.{title,hint,accessibilityLabel}`。中文与英文原文：
  - title：`允许其他主机的 Agent 调用本机` ／ `Let agents on your other hosts use this host`
  - hint：`打开后，本账号下任何主机上的 Agent 都可以在本机创建和操作 Agent，相当于信任这些主机。` ／
    `When on, agents on any host of this account can create and control agents here. Turn it on only if you trust all of those hosts.`
  - 其余语言按这两句翻译。

- [ ] **Step 1：写失败的测试**：缺省配置下开关为关；切换后以 `{ mcp: { acceptPeers: true } }` 调用 `patchConfig`；主机未连接时不渲染。
- [ ] **Step 2：** `npm run test --workspace=@ait/mobile -- --project unit src/screens/settings src/i18n/resources.test.ts` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** 同一命令加 `npm run typecheck --workspace=@ait/mobile` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(settings): accept agents from other hosts"`

### Task C3：跨主机子 Agent

**Files:**

- Modify: `apps/mobile/src/utils/agent-snapshots.ts`（归一化后的 Agent 增加 `remoteParent: { serverId: string; agentId: string } | null`）、
  `apps/mobile/src/subagents/select.ts`、`apps/mobile/src/subagents/track.tsx`、`apps/mobile/src/task-board/model.ts`
- Test: `apps/mobile/src/subagents/select.test.ts`（没有则新建）、`apps/mobile/src/task-board/model.test.ts`

**Interfaces:**

- Consumes：C1 的 `getRemoteParentFromLabels`。
- Produces：`SubagentRow` 增加 `serverId: string`（子 Agent 所在主机）。父 `(serverId P, agentId X)` 的行 = 本机 `parentAgentId === X` 的 Agent，
  加上**其他**已连接主机上 `remoteParent` 等于 `{serverId: P, agentId: X}` 的 Agent。任务总览的父任务查找：有 `remoteParent` 时用
  `taskKey(remoteParent.serverId, remoteParent.agentId)`，否则沿用现有的本机查找。行上的打开、审批、停止、归档使用行自己的 `serverId`。
  轨道里远程行显示所在主机的名称。“归档已完成的子 Agent”只处理本机行。

- [ ] **Step 1：写失败的测试**

```ts
it("lists children on another connected host under their remote parent", () => {
  /* 主机 A 有父 X；主机 B 有 Agent c，remoteParent {A, X} → X 的行含 c，row.serverId === "B" */
});

it("does not adopt a remote child under a local agent with the same id", () => {
  /* 主机 B 上也有 id 为 X 的 Agent；c 的 remoteParent 是 {A, X} → B 的 X 没有子行 */
});

it("shows a remote child on its own when its parent host is not connected", () => {
  /* 只连着 B：任务总览里 c 是顶层任务 */
});

it("rolls a remote child's status into the parent task", () => {
  /* c 等待权限 → A 上父任务的聚合状态为待处理，c 不再单独成卡 */
});
```

- [ ] **Step 2：** `npm run test --workspace=@ait/mobile -- --project unit src/subagents src/task-board/model.test.ts` → 失败。
- [ ] **Step 3：** 实现。
- [ ] **Step 4：** 同一命令加 `npm run typecheck --workspace=@ait/mobile` → 通过。
- [ ] **Step 5：** 提交：`git commit -m "feat(subagents): show children running on other hosts"`

### Task C4：Skill 与客户端文档

**Files:**

- Modify: `skills/ait-orchestration/SKILL.md`、`skills/ait-orchestration/references/operations.md`、`docs/operations/task-board.md`
- Modify: `docs/plans/README.md`（移出本项）、`docs/archive/README.md`；把 `docs/plans/cross-host-mcp.md` 与本文件移入 `docs/archive/plans/` 并加归档说明

- [ ] **Step 1：** Skill 增加一节：先 `host_list`，对 `available` 的主机用 `workspace_list`/`workspace_create` 取得 `workspaceId`，
      再带 `hostId` 调用 `agent_create`，之后用同一个 `hostId` 等待与读取；`outcome_unknown` 时先检查再重试。
- [ ] **Step 2：** 任务总览文档把“同一主机上的托管子任务”一句改为包含远程子任务的现状。
- [ ] **Step 3：** `npm run check:docs && npm run verify` → 通过。
- [ ] **Step 4：** 提交：`git commit -m "docs: cross-host orchestration skill and task board"`

---

## 验收

- 自动：三个部分各自的测试命令，以及本仓库的 `npm run verify`。
- 手工（写进 PR 描述）：中心部署后，两台登录同一账号的主机 A、B。B 打开开关；在 A 上让一个 Agent 调用 `host_list`、
  带 `hostId` 的 `workspace_list`、`agent_create` 与 `agent_wait`；核对 B 上子 Agent 的两个标签、同时连着两台主机的客户端里子 Agent 出现在父 Agent 之下；
  关闭 B 的开关后再次调用得到 `host_not_accepting`。
