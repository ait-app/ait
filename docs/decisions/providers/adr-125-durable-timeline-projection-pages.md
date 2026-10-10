# ADR-125：持久化展示投影索引与按页读取 Timeline

- 状态：已实现
- 日期：2026-10-11
- 关系：保留 [ADR-065](../workspace/adr-065-paseo-directory-and-timeline-projections.md) 的投影与游标语义，以及 [ADR-090](adr-090-timeline-entry-and-page-budgets.md) 的字节预算。

## 背景

Timeline 原本先读取、解析和投影完整历史，再截取请求页面。返回少量展示项并没有限制数据库读取和投影成本，重启后仍重复这一过程。

## 决策

- provider 继续拥有 canonical Timeline 与纯展示投影。纯投影从 RPC 内部移到 crate 私有模块，供存储索引和搜索复用。
- Timeline SQLite schema 升至版本 6，增加每个 Agent 的投影 epoch、展示组、源位置覆盖范围和标准化源条目。原始事件继续保留，派生索引可以重建。
- append、progress 和 completion 在原有事务中增量维护展示组；历史 replacement 清除并在同一事务重建相关索引。失败不发布部分索引。
- 旧数据库按 Agent 首次使用构建一次；索引随 SQLite 保存，后续查询和 daemon 重启后复用。迁移后首次构建仍需要读取该 Agent 的既有历史。
- tail/before/after 按展示位置及源覆盖索引定位，只解析所需页面。保留工具生命周期合并、文本合并、非连续 `sourceSeqRanges`、epoch、reset、gap 和游标边界。
- 超过响应预算的展示组改为读取有界标准化源窗口，再使用同一纯投影。过大的合并文本保存预算标记，完整内容留在源表，避免每个新片段重写已超限的累计文本。
- 根 Agent 和 subagent Timeline RPC 共用存储分页；subagent 存在性查询使用 SQL EXISTS。搜索、导出和原生历史校验的完整读取职责保留。

## 后果

普通页面的解析量由请求窗口决定，存储增加派生索引空间与写入维护成本。旧版本 daemon 无法直接使用版本 6 数据库。本决策没有让首次原生历史导入变成后台任务，也没有承诺实网延迟数值。

## Test coverage

未测量：用户未请求覆盖率。定向测试比较新旧响应，覆盖增量流、progress 结算、替换、游标、字节回退、旧版本迁移和事务失败；重启后破坏窗口外数据的回归验证页面读取范围。执行结果见本次交付说明。
