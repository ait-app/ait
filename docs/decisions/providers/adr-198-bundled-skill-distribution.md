# ADR-198：内置 Skill 随 daemon 启动分发

- 状态：接受
- 日期：2026-10-28
- 修订：[ADR-043](../workspace/adr-043-daemon-skills.md) 的资源发现与启动收敛；能力归属沿用 [ADR-129](adr-129-provider-owned-skills.md)。

`ait-orchestration` 需要随 desktop 和独立 daemon 一起交付。将仓库内的 Skill 正文、
参考和原生发现元数据编译进 provider 的本地安装适配器，避免依赖启动时的源码目录、
网络下载或另外部署资源包。对安装器仍使用 `orchestration` catalog 名称，由原有
`ait-` 前缀规则生成最终的 `ait-orchestration` 目录。外部 bundle 扩展目录，
同名项整体覆盖内置资源；外部项移除后重新使用内置版本。

daemon 在已有阻塞初始化阶段调用 Skills 的启动分发入口，先恢复事务，再按服务器保存的
selection 执行新增和更新。没有保存选择时沿用 `all`，但启动不写入默认选择，保留旧客户端
首次导入选择的兼容行为。启动不执行删除，待删除项继续通过现有确认流程处理。
所有权标记、三目标事务、用户额外文件保留和文件操作队列继续复用现有实现。

显式卸载保留 selection，同时将 `<data-dir>/skills-state/automatic.json` 写为 `false`，
阻止下次启动重新安装。手动 reconcile 或成功保存 selection 会重新启用自动分发。
自动分发偏好与目录及选择变更共用事务日志，失败和中断时一起回滚；新增的日志字段可缺省，
既有 `ait-v1` 日志仍可恢复。自定义空选择也是持续不安装的有效选择。

分发失败记录安全告警并继续启动 daemon，保留冲突目录、日志和备份，供用户修复后重试。
不能因一个用户技能目录不可写而使整个 daemon 不可用。此入口只分发 Skill 文件，
Provider MCP 配置注入和会话中的工具发现仍由各自流程处理。

配置、更新和停用见 [操作说明](../../operations/agent-skills.md)。
