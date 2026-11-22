---
id: ADR-297
status: accepted
date: 2026-11-22
amends:
  - workspace/adr-296-workspace-notepad.md
  - clients/adr-281-workspace-notebook-tab.md
paths:
  - crates/domain/src/workspace/notepad.rs
  - crates/model/src/workspace/notepad.rs
  - crates/persistence/src/storage/notepad.rs
  - crates/metadata/src/notepad
  - crates/api/src/mcp/tools/notepad.rs
  - crates/api/src/guest/policy.rs
  - apps/mobile/src/panels/notebook-panel.tsx
  - packages/protocol/src/workspace-notepad.ts
---

# ADR-297：记事本是数据目录下的 Markdown 文件，由文件编辑器打开

## 背景

[ADR-296](adr-296-workspace-notepad.md) 把记事本从检出目录的 `.ait-notebook.md` 移到 daemon 的 SQLite 存储，并为它保留了一个专用的
文本框面板：轮询、自动保存、冲突提示各写了一套，没有打开文件时的编辑器所具备的语法高亮、查找、Vim 键位、Markdown 预览和实时更新。
记事本本来就是一段 Markdown，放在数据库里也让用户无法用别的工具查看或备份它。

## 决策

- **存放位置。** 每个 Workspace 的记事本是 `<data-dir>/notepads/<Workspace ID>/notepad.md`，默认即 `~/.ait/notepads/...`。
  目录名由 Workspace ID 可逆地拼出：小写字母、数字、`_`、`-` 原样保留，其余字节写作 `%XX`，因此在不区分大小写的卷上不会相撞，
  也不会出现 `.` 或 `..`；ID 超过 80 字节的 Workspace 没有记事本。它仍然不在检出目录中，不进入 Git，不受重置影响。
  `persistence` 的 `FileNotepadStore` 取代 SQLite 适配器，端口仍是 `model::workspace::notepad::NotepadStore`。
- **文件即记事本。** 文件的内容就是记事本的全部状态，没有另外的元数据。`revision` 改为由正文算出的不透明字符串
  （SHA-256 的前 8 字节），正文相同则相同；`updatedAt` 取文件的修改时间；不再记录最后写入者（`updatedBy`），
  追加的段落仍在正文里写明 `[作者 · 时间]`。
- **创建与导入。** 文件在第一次 `workspace.notepad.get.request` 时创建。此时若检出目录中有非空且合法的 `.ait-notebook.md`
  （[ADR-281](../clients/adr-281-workspace-notebook-tab.md) 的存放位置），由 daemon 以它的正文作为初始内容，不修改也不删除该文件；
  客户端不再做导入。ADR-296 的 SQLite 文件不迁移：它只存在于未发布的构建中，daemon 不再读取，也不删除它。
- **读取。** `get` 返回 `path`（文件的绝对路径）、`directory`（只存放该文件的目录）、`revision`、`text` 与 `updatedAt`。
  文件大于 1 MiB（文件编辑器的上限）或不是 UTF-8 时读取失败；开头的 BOM 不计入正文。
- **写入。** `write` 的 `expected` 改为字符串，`replace` 在记事本为空时可以省略它；`append` 与正文之间保持一个空行，并以换行结尾。
  一次写入之后正文至多 256 KiB，这个上限只约束 `write`，不约束编辑器。写入以临时文件加重命名完成，重命名之前重新读取文件：
  若它已不是写入所依据的内容，则按新的内容重新决定（`append` 重试，`replace` 得到冲突），至多四次。
  这与文件编辑器保存时的条件检查一样，检查和重命名之间仍有极短的窗口。
- **客户端。** 记事本 tab 不再有自己的编辑器：它用 `get` 取得 `directory` 与文件名，然后渲染与打开文件相同的文件面板
  （`FilePane`），以 `directory` 作为 `cwd`。读取、`fs.file.subscribe` 实时更新、带 `revision` 条件的保存、冲突提示与 Markdown 预览
  都由文件面板提供；Web 与桌面端默认进入源码编辑，原生移动端的文件面板没有编辑器，记事本在那里是只读的渲染预览。
  取不到路径时显示错误和“重试”。
- **访客。** 访客的文件请求原本要求 `cwd` 在所分享的检出目录内。`fs.explorer.request`、`fs.file.subscribe.request` 与
  `fs.file.write.request` 现在也接受某个记事本目录作为 `cwd`，条件是该目录所属的 Workspace 在分享范围内；其他文件、检出与下载方法
  仍然拒绝记事本目录。`workspace.notepad.*` 两个方法的放行规则不变。
- **MCP。** `notepad_read` 多返回 `path`，不再返回 `updatedBy`；`notepad_write` 的 `expected` 为字符串。工具仍是 Agent 写记事本的方式：
  沙箱化的 Agent 写不到数据目录，而且 `append` 的署名与条件写入只有经过 daemon 才有。

## 后果

- 人在 tab 里得到的就是文件编辑器；Agent 经工具写入后，打开着的 tab 通过文件订阅立即看到，没有轮询。
- 记事本可以用任何工具查看、编辑和备份；直接改文件与经编辑器保存等价。
- 用户在编辑器里可以写入超过 256 KiB 的正文，此后 `notepad_write` 的 `append` 会被拒绝，直到正文被缩短。
- 不再知道整段替换是谁写的；需要署名的内容用 `append`。
- 原生移动端暂时不能编辑记事本，直到文件面板在那里支持编辑。
- 访客可以经文件方法覆盖所分享 Workspace 的记事本文件，这与此前经 `workspace.notepad.write.request` 能做的相同。
