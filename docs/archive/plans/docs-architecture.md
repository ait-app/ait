# 文档架构整理

来源：2026-11-10 对 `docs/` 的结构核查（源码提交 `7d110395`）。下列数字均为当日对仓库的实测，
每项写明现状、影响和建议做法，完成后勾选并附上修改。

文档的主要读者是在 Worktree 中工作的 Agent。现状是历史多、现行事实少：
`decisions/` 216 篇约 1.1 万行，`reports/` 213 篇，而[当前架构](../../architecture/README.md)只有 145 行。
Agent 检索文档时多数命中落在历史记录上。

## 待办

- [x] **ADR 编号持续重号。**
      16 个编号各有两份；[ADR 索引](../../decisions/README.md)只说明了其中 12 个，211、215、219、223 未列出，
      而这四个都产生在[文档规范](../../policy/documentation.md)要求“使用尚未占用的递增编号”之后。
      并行 Worktree 各自取“下一个编号”，靠规范无法避免。建议改为合并时分配编号，或使用日期加 slug 作为标识；
      在此之前先把索引的重号说明补齐。
      已处理：`npm run check:docs` 只允许这 16 个历史重号各两份，任何新重号都失败；索引说明补齐 16 个编号。编号分配方式（合并时分配或日期加 slug）仍待负责人决定。
- [x] **ADR 状态无法查询。**
      状态行有三十多种写法（`Accepted`、`已实现`、`接受`、`已接受`、`采用`，其余是附带日期或说明的变体）；
      151 篇提到修订或取代，只有 4 篇标记为 `Superseded`。无法回答“某个 crate 当前生效的决策有哪些”。
      建议为 ADR 增加 front matter（编号、状态枚举、取代关系、所属 crate 路径），索引由脚本生成，
      并让 `npm run check:docs` 校验编号唯一、状态取值和索引完整；该检查目前只验证本地链接。
      已处理：216 篇 ADR 均有 front matter（`id`、`status` 四值枚举、`date`、`supersedes`、`amends`、`paths`），由脚本从原状态行和关系行生成，附加说明保留为“状态说明”；[ADR 索引](../../decisions/README.md)由 `node scripts/adr.mjs index` 生成，`check:docs` 校验字段、状态、关系目标、取代一致性和索引是否最新。
- [x] **两条规则互相矛盾。**
      [仓库约定](../../../AGENTS.md)第 13 行要求每个 ADR 都更新 `docs/README.md`，文档规范则要求该文件只做导航、
      不逐条复制 ADR。结果是 `docs/README.md` 第 35 行长 925 个字符，串联了七个 Bot ADR 链接。
      建议把仓库约定改为“更新分类索引和架构概览”，并清理 `docs/README.md` 中逐条列出的 ADR。
      已处理：仓库约定改为重新生成分类索引并更新架构概览和对应规格；`docs/README.md` 删除逐条 ADR 链接，每个主题只保留操作页或规格入口，原有操作页与架构页入口全部保留。
- [x] **缺少按能力划分的现行规格。**
      要了解 Provider 当前的约束，只能通读 72 篇 Provider ADR。建议为 provider、workspace、bot、
      连接与传输各维护一份现行规格，写明生效的不变量；ADR 被接受时必须把结论并入规格，ADR 本身只保留决策过程。
      已处理：新增 [Provider](../../architecture/provider.md)、[Workspace](../../architecture/workspace.md)、[Bot](../../architecture/bot.md)、[连接与传输](../../architecture/connection.md) 规格，每条不变量对照当前源码核实并引用（修订后的）ADR，从[架构概览](../../architecture/README.md)链接。
- [x] **ADR 门槛过低。**
      10 月新增 194 篇，约每天 6 篇，多数是功能设计或变更说明而不是边界决策。建议 ADR 只记录边界变化，
      功能设计写入规格或操作手册。
      已处理：[文档规范](../../policy/documentation.md#adr-门槛)写明 ADR 只记录持久边界变化，功能设计写入规格或操作文档，并各给一个例子。
- [x] **验证报告的去向。**
      `reports/` 在 10 月新增 238 篇，11 月至今 8 篇，这一做法实际上已经停止。报告记录的是单次 PR 的验证证据，
      应写在 PR 描述中；现有报告建议一次性整体归档，避免旧测试数字被当作当前结果。
      已处理：368 个报告及 JSON 附件用 `git mv` 按原分类移入[历史报告](../reports/README.md)，脚本修复全部入站和出站相对链接；规范与 `reports/README.md` 改为把逐 PR 验证证据写在 PR 描述中。
- [x] **代码几乎不指向决策。**
      全部 Rust 源码中只有 13 处 ADR 引用。修改某个文件的 Agent 无法从代码找到约束它的决策。
      生成索引具备“所属 crate 路径”后，可以按路径反查，不必在代码中逐处补注释。
      已处理：`node scripts/adr.mjs governs <路径>` 按 front matter 的 `paths` 前缀列出现行 ADR（`--all` 含已取代），仓库约定中有一行说明。

## 已处理

- 已实现的 Workspace Bot 阶段 1–3 实施计划原先留在 `plans/`，2026-11-10 已移入[归档](../README.md)。
