# 文档维护规范

每份文档应有明确用途和归属。当前说明对照源码维护；历史证据保留当时的范围，不能混作操作指南。

## 分类与入口

| 目录            | 内容                                                               |
| --------------- | ------------------------------------------------------------------ |
| `architecture/` | 当前组件职责、依赖与数据所有权；按能力的现行规格                   |
| `decisions/`    | 按 daemon、workspace、providers、clients、bot、branding 分类的 ADR |
| `operations/`   | 当前可执行的配置、诊断、构建与发布流程                             |
| `policy/`       | 工程约束与贡献规范                                                 |
| `plans/`        | 尚未实施或未完成验收的工作                                         |
| `reports/`      | 只保留说明；验证证据写在 PR 描述中，旧报告已归档                   |
| `archive/`      | 已完成或被取代的计划、早期迁移与旧版本报告及附件                   |
| `assets/`       | 当前决策仍引用的设计资源                                           |

根 README 负责产品和快速开始；`docs/README.md` 只提供按任务的导航，不再逐条复制 ADR、版本和报告。
完整 ADR 清单归 `decisions/README.md`（脚本生成），历史报告清单归 `archive/reports/README.md`，未完成事项归 `plans/README.md`。
相同流程选择一份权威操作文档，其余入口链接到它；共享包和客户端 README 只补充本目录特有内容。

## 更新与归档

1. 达到[门槛](#adr-门槛)的修改增加 ADR，重新生成 ADR 索引，并更新架构概览和对应的现行规格：
   规格写结论，ADR 只保留决策过程。单纯文档整理无需新增架构决策。
2. 新 ADR 使用尚未占用的编号，包含 [front matter](#adr-元数据)、标题、背景、决策和后果；
   修订或取代旧决策时在 front matter 中登记关系，正文说明影响的条目。
   已有重号按完整路径区分，不为整理文档重编号；改为合并时分配编号还是日期加 slug 尚未决定。
3. 操作命令对照当前脚本和工作流；平台、权限、生命周期和默认值对照源码。
   数量、版本和动态能力优先引用查询入口或权威清单，避免复制易过期数字。
4. 计划完成或被取代后，把仍未完成的工作提取到当前计划，再将原方案归档，标明原因与替代入口。
   保留有价值的历史命令、测量数据及关联附件；无独立追溯价值的重复说明可直接合并。
5. ADR 不按年龄机械归档：仍有效的边界保留原路径，被取代的决策标明后继。
   第三方来源、许可证和当前资产不能作为普通旧文档清理。
6. 单次 PR 的验证证据（源码提交、平台、命令、范围、限制，测试结果与覆盖率分开）写在 PR 描述中，
   不再新增 `reports/` 文件；仍需长期引用的测量结论并入规格或操作文档。
   [历史报告](../archive/reports/README.md)不代表当前结果；Rust 验证遵循[仓库约定](../../AGENTS.md)与 [Rust 规范](rust.md)。

## ADR 门槛

ADR 只记录持久边界的变化：crate 或模块的归属与依赖方向、跨组件契约（协议、端口、存储格式）、
安全或权限边界，以及需要多处代码共同遵守的生命周期规则。功能设计、单个适配器的行为细节、
参数调优和缺陷修复写进对应的现行规格或操作文档，并在 PR 描述中说明。

- 需要 ADR：“在线业务连接只使用 Hub，不再回退 Relay”（[ADR-204](../decisions/clients/adr-204-hub-only-online-business-transport.md)）
  改变了所有客户端与 daemon 之间的传输契约。
- 不需要 ADR：调整某个 Provider 的刷新间隔或冷启动预算，写入 [Provider 规格](../architecture/provider.md)
  或该 Provider 的操作文档即可。

## ADR 元数据

每个 `decisions/<分类>/adr-<编号>-<slug>.md` 以 front matter 开头：

```yaml
---
id: ADR-227
status: accepted
date: 2026-11-10
supersedes: []
amends:
  - bot/adr-213-single-workspace-manager-bot.md
paths:
  - crates/bot
  - crates/provider/runtime
---
```

- `id` 与文件编号一致；`date` 为决策日期（`YYYY-MM-DD`）。
- `status` 取 `proposed`（未接受）、`accepted`（生效，包括已实现）、`superseded`（整体被取代）或
  `withdrawn`（撤销且无后继）。附带说明写在正文的“状态说明”行，不放进枚举。
- `supersedes` 列出被本决策整体取代的 ADR，对方状态必须为 `superseded`；`amends` 列出被本决策部分修订、
  部分取代或扩展的 ADR，对方仍生效。两者写相对 `decisions/` 的路径，可省略空列表。
- `paths` 列出受决策约束的仓库路径（crate、应用、包或工作流目录），必须存在，`adr.mjs governs` 按前缀反查。

`decisions/README.md` 由 `node scripts/adr.mjs index` 生成，只手工维护首个二级标题之前的说明。
`npm run check:docs` 拒绝缺少字段、未知状态、不存在的关系目标或路径、取代关系与状态不一致、
索引过期，以及新的重复编号；历史上的 16 个重号记录在 `scripts/adr.mjs` 中，不再增加。

## 文档变更检查

移动文档时修正入站链接、文档自身的相对路径和相关附件引用；更新所属分类索引。
运行 `npm run check:docs` 检查本地链接与 ADR 元数据，并对改动文件运行适用的格式检查与 `git diff --check`。
链接检查不访问外部网站，也不验证页面锚点，修改标题时需另行检查锚点引用。
仅修改文档且未改动 `bins/` 或 `crates/` 的 Rust 代码时，不运行 Rust 测试。
