---
id: ADR-230
status: accepted
date: 2026-11-10
paths:
  - crates/metadata/src/automation
  - crates/api/src/connection/workspace_archive.rs
---

# ADR-230：归档 Worktree 时运行 teardown

- 关系：补全 `ait.json` 中已校验但未执行的 `worktree.teardown`；沿用 setup 的限制与信任门禁，见 [Workspace 自动化](../../operations/workspace-automation.md)。

## 背景

项目设置可以保存 `worktree.teardown`，daemon 只校验不执行，归档 Worktree 时无法清理 setup 创建的外部资源
（容器、缓存登记、端口占用等）。

## 决策

1. **时机。** 仅当归档会删除 Ait 拥有的 Worktree 检出时运行：在 Agent、终端和脚本关闭之后、删除检出之前。
   `metadata::worktrees` 新增 `teardown_workspace(&PendingArchive)`，在检出仍被其他未归档 Workspace 引用时返回空；
   `finish_archive` 删除前仍重新检查引用。适用于 `workspace.worktree.archive.request`、`workspace.archive.request`
   与 `project.remove.request`。普通目录 Workspace 不运行 teardown，与 setup 只在新建 Worktree 时运行对称。
2. **环境。** 使用创建该 Worktree 的 Workspace（否则最早的选中 Workspace）的 `cwd`、`ait.json` 和变量；
   Workspace 端口与 setup 相同。
3. **限制与安全。** 与 setup 相同：命令依次运行、遇到第一个失败即停止，每条最长 30 分钟、输出不超过 8 MiB，
   超限终止进程组。不受信且未批准的 Workspace（fork PR 检出）不运行 teardown，不会因归档而执行未经批准的代码。
4. **执行位置。** automation 运行时只准备 `TeardownJob`；API 在阻塞线程中运行它，不持有 worktree 或 automation
   服务锁，也不占用短任务预算，归档请求等待其完成。
5. **失败语义。** teardown 失败（包括配置无效、命令失败、超时）不阻止归档：记录 `warn` 日志（Workspace ID 与
   失败的命令序号／退出码，不含输出），然后照常删除检出。理由：归档也由合并后自动归档等非交互路径触发，
   失败的 teardown 若阻止归档，会让检出无法删除且用户只能先修改配置；setup 失败同样不撤销已创建的检出。
6. **重复执行。** 其他清理失败导致归档重试时，teardown 可能再次运行；文档要求 teardown 可重复执行。

## 后果

- teardown 输出不保存，失败只在 daemon 日志中可见；需要展示时再决定位置。
- 归档请求的耗时包含 teardown，最长可达各命令上限之和。

## 验证

`metadata` 单元测试覆盖命令顺序、首个失败即停止、启动失败、无配置、无效配置、不受信 Workspace 跳过和
teardown 目标选择；daemon 进程测试归档真实 Git Worktree，确认 teardown 在删除前以 `AIT_BRANCH_NAME` 运行。
