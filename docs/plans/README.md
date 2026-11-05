# 未完成工作

这里只保留尚需实施或验收的工作；已完成的设计与测量移入[归档](../archive/README.md)。
计划不代表已交付功能，历史测试结果也不代表当前版本已验证。

2026-11-04 的待办核查、修复范围和定向检查见[验证记录](../reports/daemon/plans-todo-followup-2026-11-04.md)。

| 工作                                                            | 当前状态与下一步                                                                                                                               |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| [仓库审查遗留事项](repository-review-2026-11-02.md)             | 已修复已确认条目；剩余真实 OpenCode 子会话通知与中心拒绝访客路由行为确认                                                                       |
| [daemon 审查遗留事项](daemon-review-2026-11-03.md)              | 实例锁顺序与状态写入已补齐；剩余语音和定时任务整体超时的产品语义                                                                               |
| [移动端审查遗留事项](mobile-review-2026-11-04.md)               | 已确认缺陷已修复；剩余排队附件编辑、非 QWERTY 快捷键、侧边栏刷新指示与更新检查去重                                                             |
| [远程 Workspace 打开性能](remote-workspace-open-performance.md) | 已有并发隔离、目标读取与页级投影；仍需实网基线和剩余优化                                                                                       |
| [客户端登录续期](relay-client-session-persistence.md)           | 凭据可持久化；用户会话自动续期尚未实现                                                                                                         |
| 移除 `paseo` 工具名别名（0.2.0）                                | 0.2.0 发布时从 TS `AIT_TOOL_NAMESPACES`、移动端 `DIRECT_AIT_TOOL_PREFIXES` 与 Rust fork context 中删除 legacy `paseo` 命名空间，并更新相关测试 |
| [Workspace Bot 管理者重设计](workspace-bot-manager.md)          | 设计已批准；[阶段 1](workspace-bot-manager-phase-1.md)（单 Bot 核心模型，[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)）已实现、待合并；阶段 2–4（客户端、权限与 MCP、来源与通知）待实施                                          |
| Bot 后续能力                                                    | [内置、Telegram、飞书已接入](../operations/bots.md)；真实平台验收仍需独立记录，Email、流式订阅及 uncertain 投递恢复未实现                      |

Bot 的 Email 线程、去重和防循环约束保留在[原方案](../archive/plans/bot-backend.md#email-可行性)。
后续实施前重新确认范围、协议与平台限制，不能把可行性方案当作可用 Channel。
