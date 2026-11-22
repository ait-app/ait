---
id: ADR-296
status: accepted
date: 2026-11-22
amends:
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-280-bot-recall-reads-and-spill.md
paths:
  - crates/domain/src/workspace/notepad.rs
  - crates/model/src/workspace/notepad.rs
  - crates/persistence/src/storage/notepad.rs
  - crates/metadata/src/notepad
  - crates/api/src/mcp
  - crates/api/src/guest
  - apps/mobile/src/panels/notebook-panel.tsx
---

# ADR-296：每个 Workspace 一份由 daemon 保存的记事本

## 背景

同一 Workspace 的 Agent 之间没有被动的共享通道：`agent_prompt` 是点对点的并且会开启对方新的一轮，`ticket_report` 只从工作者到 Bot，
Bot 记忆只对 Bot 会话可见。客户端有一个实验性的记事本 tab，把人写的笔记存放在检出目录的 `.ait-notebook.md`，通过文件协议读写；
它只在有人打开 tab 后才存在，会进入 Git 工作区、被 `checkout.reset_workspace.request` 清除，Agent 也不知道它。

记事本应该是 Workspace 自身的一部分：不论 tab 是否打开都存在，人和 Agent 读写的是同一份，每个 Workspace 有且只有一份。

## 决策

- **模型。** 每个 Workspace 有且只有一份记事本：一段文本，带 `revision`、最后写入者与时间。它随 Workspace 存在，从未写入时为空文本、
  `revision` 为 0，不需要创建。正文至多 256 KiB，不允许 NUL。纯值与状态迁移在 `domain::workspace::notepad`。
  记事本内没有按 key 的便签，也没有单独的权限分组：能在这个 Workspace 工作的调用者就能读写它的记事本。
- **写入。** `replace`（默认）设置整段文本，要求 `expected` 等于当前 `revision`，只有从未写入的记事本可以省略；允许写入空文本以清空。
  `append` 在末尾追加一段，段前由服务端写入 `[作者 · 时间]`，不需要 `expected`，追加的文本不能为空。每次写入使 `revision` 加 1。
  Agent 应优先使用 `append`，它不会覆盖别人写的内容。
- **拒绝是结果而不是错误。** 修订号不符或超长返回 `accepted: false`、`error` 文本、当前 `revision` 与最后写入者；
  未知或已归档的 Workspace 返回 `WorkspaceNotFound`，含 NUL 的正文、空的追加或非法作者返回 `InvalidMessage`。
- **归属与存储。** `workspace.notepad.get.request` 与 `workspace.notepad.write.request` 归 `metadata` 的 `notepad` 能力组
  （[ADR-130](adr-130-workspace-methods-owned-by-metadata.md)）。存储端口为 `model::workspace::notepad::NotepadStore`，由 `persistence` 的
  SQLite 适配器实现，文件为 `<data-dir>/notepads/notepads.sqlite3`，每个 Workspace 一行
  （打开数据库的方式与创建回执共用，[ADR-294](../daemon/adr-294-sqlite-creation-receipts.md)）。
- **生命周期。** 记事本在 daemon 重启后保留，不在检出目录中，因此不进入 Git、不受重置影响，沙箱化 worktree 也不需要为它放开写入路径。
  Workspace 归档期间记事本不可读写但保留，恢复后内容仍在；Workspace 记录被移除时，服务通过登记表的变更订阅删除它。
  记事本不随 worktree 创建复制，托管 worktree 是独立的 Workspace，有自己的记事本。
- **作者。** `write` 接受可选的 `author`（`{kind, agentId?, title?}`，`kind` 为 `agent`、`bot`、`user`、`external`），缺省为 `user`。
  MCP 层填写它：Bot 会话为 `bot`，子树凭据为该 Agent，主机凭据取 `X-Ait-Agent-Id` 指向的 Agent，没有时为 `external`。
  该请求头不是权限边界（[ADR-148](../daemon/adr-148-daemon-mcp.md)、ADR-215），持有主机凭据的调用者也可以直接调用 RPC 并填写任意作者，
  所以署名只是说明，不能当作身份证明。
- **MCP 工具。** `notepad_read`（按字符分页，默认 4000、至多 8000）与 `notepad_write`，命名与参数遵循
  [ADR-295](../daemon/adr-295-mcp-tool-naming-and-aliases.md)。`workspaceId` 省略时取调用者 Agent 的 Workspace；
  主机凭据可以显式指定任意未归档的 Workspace。
- **Bot 作用域（修订 ADR-215）。** 两个工具都属于总是授予的 `observe`，不新增权限分组；目标必须在授权的 Workspace 内，
  `workspaceId` 省略时子树 Agent 取自己的 Workspace，Bot 会话取根 Workspace。Bot 指令说明用法，
  `bot_overview` 返回根 Workspace 记事本的修订号与字符数。
- **受限于外部对话的调用者（修订 ADR-280 第 4 节）。** 其他 Channel 的对话及为它工作的 Agent 只能触达自己工单的 Agent；记事本是整个 Workspace
  共享的，会让不同对话的内容互相可见，所以它们调用两个工具都得到引导文本，`bot_overview` 中的记事本为 `null`。
  这沿用已有的对话隔离，不是记事本自己的权限概念。
- **访客。** 两个方法按 Workspace 分享放行：能访问该 Workspace 的访客可以读写它的记事本，与此前经文件权限读写 `.ait-notebook.md` 一致。
  访客请求中的 `author` 被移除，写入一律记为 `user`。
- **客户端。** 记事本 tab 改为读写本记事本：加载用 `get`，可交互时每 3 秒轮询一次并在没有未保存修改时采用较新的文本，
  自动保存用带 `expected` 的 `replace`；修订号不符时进入冲突状态并提供“重新加载”。`ServerInfo` 的 `workspaceNotepad` 特性在两个方法都已实现时为真，
  否则 tab 提示需要更新主机。首次加载时若记事本从未写入而检出目录中存在非空的 `.ait-notebook.md`，客户端把它导入一次，不修改也不删除该文件。
- **信任。** 记事本的内容是其他 Agent 与用户写的数据，不是指令：一个读过恶意内容的 Agent 可以把诱导文本写进去。工具描述、`initialize` 的
  `instructions`、Skill 与 Bot 指令都这样说明，并要求不写入凭据。

## 后果

- 人和 Agent 看到的是同一份记事本，tab 不打开它也存在。
- 写入不唤醒任何 Agent，提问要等对方下次读取，或由编排者用 `agent_prompt` 提醒。
- 整段替换以修订号互斥：人在 tab 中编辑时若 Agent 先写入，保存会冲突，需要重新加载后再改。Agent 使用 `append` 不会冲突。
- 没有变更事件，tab 靠轮询看到别人的写入，最多延迟一个轮询间隔。
- 记事本不是访问控制边界：同一 Workspace 内的调用者互相可以覆盖。
- `.ait-notebook.md` 不再被读写；导入之后留在检出目录中，由用户自行处理。

## 未包含

- 变更事件与订阅、向指定 Agent 投递、服务端记录每个 Agent 的已读位置、Project 级记事本、按对话隔离的记事本。
