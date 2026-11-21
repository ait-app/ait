---
id: ADR-296
status: accepted
date: 2026-11-21
amends:
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-280-bot-recall-reads-and-spill.md
paths:
  - crates/domain/src/workspace/notepad.rs
  - crates/model/src/workspace/notepad.rs
  - crates/persistence/src/storage/notepad.rs
  - crates/metadata/src/notepad
  - crates/api/src/mcp
  - crates/domain/src/bot/permissions.rs
---

# ADR-296：Workspace 记事本（notepad）：同一 Workspace 的 Agent 共享的便签

## 背景

同一 Workspace 的 Agent 之间没有被动的共享通道。`agent_prompt` 是点对点的并且会开启对方新的一轮；`ticket_report` 只从工作者到 Bot；
Bot 记忆只对 Bot 会话可见；检出目录中的文件会进入 Git 工作区，被提交、被 `checkout.reset_workspace.request` 清除或与改动冲突。
Agent 需要一个地方留下别人要用的信息：约定的接口、已经做出的决定、正在占用的文件、问题与答复。

客户端已有一个实验性的 notebook tab，把人写的笔记存放在检出目录的 `.ait-notebook.md`（[Workspace 规格](../../architecture/workspace.md#客户端-workspace-记事本)）。
它不经过 daemon 的业务能力，也没有署名、修订或对 Bot 作用域的限制。本决策的记事本是另一份数据；两者是否合并尚未决定，见“未包含”。

## 决策

- **模型。** 每个 Workspace 一份记事本，按 `workspaceId` 寻址，由按 `key` 寻址的便签组成。`key` 为 1–64 个字符的 `a-z`、`0-9` 与 `-`，
  以字母或数字开头，由调用者选择，使 Agent 在上下文被压缩后仍能凭名字找回便签。便签带 `text`、从 1 起的 `revision`、创建者与最后写入者、
  创建与最后写入时间。记事本自身的 `revision` 在任何便签创建、修改、删除时加 1，每条便签记录最后一次变化时的记事本修订号，供 `since` 过滤。
  上限为每个 Workspace 64 条、每条 16 KiB，不允许 NUL，达到上限时拒绝而不淘汰。纯值与状态迁移在 `domain::workspace::notepad`。
- **写入。** `replace`（默认）在便签不存在时创建，已存在时要求 `expected` 等于当前 `revision`；`append` 在末尾追加一段，段前由服务端写入
  `[作者 · 时间]`，不需要 `expected`，便签不存在时创建。删除要求 `expected`。任何能写的调用者都能修改和删除任何便签：
  `expected` 防止的是无意覆盖，不是权限隔离。
- **拒绝是结果而不是错误。** 修订号不符、缺少 `expected`、便签不存在、已满或超长返回 `accepted: false`、`error` 文本，以及找到便签时它的当前
  `revision` 与 `updatedBy`；未知或已归档的 Workspace 返回 `WorkspaceNotFound`，非法的 `key`、正文或作者返回 `InvalidMessage`。
- **归属与存储。** `workspace.notepad.list.request`、`.get.request`、`.write.request`、`.delete.request` 归 `metadata` 的 `notepad` 能力组
  （[ADR-130](adr-130-workspace-methods-owned-by-metadata.md)）。存储端口为 `model::workspace::notepad::NotepadStore`，由 `persistence` 的
  SQLite 适配器实现，文件为 `<data-dir>/notepads/notepads.sqlite3`，每次变更在一个事务中写入便签与记事本修订号
  （打开数据库的方式与创建回执共用，[ADR-294](../daemon/adr-294-sqlite-creation-receipts.md)）。
- **生命周期。** 记事本在 daemon 重启后保留，不在检出目录中，因此不进入 Git、不受重置影响，沙箱化 worktree 也不需要为它放开写入路径。
  Workspace 被归档或移除时，服务通过登记表的变更订阅删除它的记事本；恢复归档的 Workspace 得到空记事本。记事本不随 worktree 创建复制。
- **作者。** `write` 接受可选的 `author`（`{kind, agentId?, title?}`，`kind` 为 `agent`、`bot`、`user`、`external`），缺省为 `user`。
  MCP 层填写它：Bot 会话为 `bot`，子树凭据为该 Agent，主机凭据取 `X-Ait-Agent-Id` 指向的 Agent，没有时为 `external`。
  该请求头不是权限边界（[ADR-148](../daemon/adr-148-daemon-mcp.md)、ADR-215），持有主机凭据的调用者也可以直接调用 RPC 并填写任意作者，
  所以署名只是说明，不能当作身份证明。
- **MCP 工具。** `notepad_list`、`notepad_read`（按字符分页，默认 4000、至多 8000）、`notepad_write`、`notepad_delete`，
  命名与参数遵循 [ADR-295](../daemon/adr-295-mcp-tool-naming-and-aliases.md)。`workspaceId` 省略时取调用者 Agent 的 Workspace；
  主机凭据可以显式指定任意未归档的 Workspace。
- **Bot 作用域（修订 ADR-215）。** 读取属于 `observe`；写入与删除属于新的权限分组 `notepad`，属于默认分组。升级前保存过权限的 Bot 不会自动获得它。
  目标必须在授权的 Workspace 内，`workspaceId` 省略时子树 Agent 取自己的 Workspace，Bot 会话取根 Workspace。
  Bot 指令在授予该分组时说明用法，`bot_overview` 返回根 Workspace 记事本的修订号与便签数。
- **受限于外部对话的调用者（修订 ADR-280 第 4 节）。** 其他 Channel 的对话及为它工作的 Agent 只能触达自己工单的 Agent；记事本是整个 Workspace
  共享的，会让不同对话的内容互相可见，所以它们调用四个工具都得到引导文本，`bot_overview` 中的记事本为 `null`。
- **访客。** 访客策略对未登记的方法默认拒绝，记事本方法不登记，Workspace 访客与整机访客都不能访问。
- **信任。** 便签是其他 Agent 与用户写的数据，不是指令：一个读过恶意内容的 Agent 可以把诱导文本写进记事本。工具描述、`initialize` 的
  `instructions`、Skill 与 Bot 指令都这样说明，并要求不写入凭据。
- **客户端。** `ServerInfo` 的 `workspaceNotepad` 特性在四个方法都已实现时为真；客户端只在此时提供 `notepad` 权限分组并把它放进新 Bot 的默认权限，
  因为较早的 daemon 会拒绝未知的分组。

## 后果

- 同一 Workspace 的 Agent 可以不打断彼此地共享信息；托管 worktree 是独立的 Workspace，默认各用各的记事本。
- 写入不唤醒任何 Agent，提问要等对方下次读取，或由编排者用 `agent_prompt` 提醒。
- 记事本不是访问控制边界：同一 Workspace 内能写的调用者互相可以覆盖和删除。
- 归档会丢弃记事本，需要保留的结论应写进仓库或工单。

## 未包含

- 客户端界面与变更事件。notebook tab 仍读写 `.ait-notebook.md`，人看不到 Agent 写的便签；是否让它改为显示本记事本，或让本记事本的工具改为
  读写那个文件，需要单独决定。
- 向指定 Agent 投递便签、服务端记录每个 Agent 的已读位置、Project 级记事本、按对话隔离的记事本。
