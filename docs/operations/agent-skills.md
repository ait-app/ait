# Agent Skill 自动分发

daemon 内置 [ait-orchestration](../../skills/ait-orchestration/SKILL.md)，包含 Agent 编排、
Workspace、Profile、终端和调度的使用指引。资源编译进二进制，desktop 与独立 daemon
使用相同分发流程，无需下载或额外复制仓库目录。

daemon 启动时先恢复未完成安装，再按已保存的 Skill 选择安装和更新。没有保存选择时默认为
`all`；自定义选择只安装指定项，空集合不安装。启动完成后，新建的 Provider 会话可通过
原生 Skill 发现机制读取文件；已有会话是否刷新由 Provider 决定。

## 安装位置与扩展

目标根目录为 `AIT_SERVER_SKILLS_HOME`，默认使用 `HOME`；两者均缺失时使用
`<data-dir>/agent-home`。固定安装到该根目录下：

- `.agents/skills/ait-orchestration/`
- `.claude/skills/ait-orchestration/`
- `.codex/skills/ait-orchestration/`

外部 bundle 根目录由 `AIT_SERVER_SKILLS_BUNDLE` 指定，默认 `<data-dir>/skills-bundle`，
其直接子目录扩展内置目录。同名外部项整体覆盖内置 Skill；例如 `orchestration/` 可覆盖
内置编排技能，移除覆盖项后恢复内置版本。安装器为 catalog 名称添加 `ait-` 前缀，
因此外部目录应名为 `orchestration`，不要使用 `ait-orchestration`。

安装选择保存在 `<data-dir>/skills-state/selection.json`。`all` 会在后续启动时自动安装
新增 bundle 项；`custom` 保留未知名称，直到对应资源出现。升级 daemon 后，已选中的内置
Skill 会在下次启动时更新，无需运行中的源码目录。

## 更新与停用

现有 `agent.skills.*` 接口继续管理这些文件：

- `get_status` 查看可用、已安装和待处理项。
- `reconcile` 手动安装、修复和更新，同时恢复后续启动自动分发。
- `save_selection` 保存选择并同步；删除目录仍需要 `confirmedRemovals`。
- `uninstall` 删除受管 Skill，保留选择，并暂停后续启动自动分发。
- `import_legacy_selection` 仍只在没有服务器选择时导入，不立即修改文件。

暂停状态保存在 `<data-dir>/skills-state/automatic.json`；手动 reconcile 或成功保存选择
会重新启用。选择自定义空集合可以长期保持不安装。

自动分发只新增和更新，不自动删除待确认目录。用户额外文件继续保留；Skill 中受管的同名
文件按 bundle 更新。未受管的同名目录会触发冲突，原文件保持不变。失败时 daemon 记录
`automatic Agent skill distribution failed` 告警并继续启动；先检查冲突与事务备份，修复后
通过 reconcile 重试。

Skill 文件分发不注册 MCP 工具，也不提供连接凭证。MCP 服务和 Provider 注入状态见
[MCP 操作说明](mcp.md)，分发决策见 [ADR-198](../decisions/providers/adr-198-bundled-skill-distribution.md)。
