# 文档后续事项

来源：2026-11-10 文档架构整理（已[归档](../archive/plans/docs-architecture.md)）过程中的发现。
下列各项由执行整理的 Agent 对照源码提出，尚未经第二人核实；每项先核实，再修改文档，完成后勾选。

## 现行规格需要复核

- [ ] **四份现行规格未经熟悉该领域的人审阅。**
      [provider](../architecture/provider.md)、[workspace](../architecture/workspace.md)、
      [bot](../architecture/bot.md) 与[连接](../architecture/connection.md)各约 30 条不变量，由 Agent 逐条对照源码写成，
      只抽查过少数常量。在被当作权威之前需要逐份审阅。
- [ ] **ADR 元数据中的推断值。**
      十篇 ADR 的日期取自 Git 历史；约 25 篇的所属路径把握不大（如 `daemon/adr-107`、`adr-116` 标为整个 `crates`）；
      `clients/adr-172`、`daemon/adr-038` 由 Accepted 改标为 superseded。需要确认或修正。

## 文档与源码不一致

- [ ] [daemon 手册](../operations/daemon.md)称 DeepSeek Harness 使用 stdio ACP；源码默认使用原生 Web Host，
      仅在 `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 时使用 ACP。
- [ ] [Bot 手册](../operations/bots.md)与 ADR-213 称移除 Bot 只释放 Telegram、飞书账号；源码软删除全部外部 Channel。
      ADR-213 另有三处：setup 幂等性还比较权限、账号唯一性也覆盖已停用的 Channel、访客可读方法清单过期。
- [ ] ADR-211、ADR-049 仍写直连与 SSH 保留四条连接，ADR-218 已改变这一点但 ADR-211 没有修订说明。
- [ ] ADR-140、180、200、201 与[无人值守 daemon](../operations/headless-daemon.md)仍提到下载 Relay 或“两条链路”；
      ADR-180 的 3 秒 pong 规则与 15 秒心跳不符。
- [ ] ADR-060 称不支持 Forge PR checkout，源码已支持。
- [ ] ADR-125 写 schema 版本 6，源码为 7。
- [ ] ADR-068 的模块路径过期；ADR-069 称 metadata 声明 `WorkspaceGitObserver`，实际位于 `model::workspace::git`。
