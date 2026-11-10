---
id: ADR-060
status: accepted
date: 2026-09-28
paths:
  - crates/metadata
---

# ADR-060：统一 Workspace 创建入口支持 Worktree

- 关联：ADR-030、ADR-037、ADR-056、ADR-058

## 背景

App 沿用 Paseo 的 `workspace.create.request`，通过 `source.kind` 选择 directory 或
worktree。Rust 已有独立的 `workspace.worktree.create.request`，但统一入口拒绝 worktree，
导致新建工作区界面无法使用已有 Git 能力。

## 决策

1. Metadata 继续拥有统一创建流程、参数校验、持久创建回执和订阅；新增由消费方定义的
   `WorktreeProvisioning` port。Filesystem 用现有 `Worktrees` 服务实现适配器，依赖方向
   仍为 filesystem → metadata。API 组合根让两个入口共享同一服务和互斥锁。
2. 支持 Paseo 的 `cwd`／`projectId`、`action`、`refName`、`baseBranch`、`branchName`、
   `worktreeSlug`。仅指定 projectId 时使用该项目根目录。显式项目必须存在且未归档。
   新分支名独立于目录 slug，refName 优先于默认 baseBranch；checkout 使用 refName。
3. Worktree 注册采用创建协调器预留的 workspaceId，保留 title 和 firstAgentContext，
   并拒绝覆盖已有 workspaceId。注册失败沿用已有 Git 回滚。相同幂等请求只回放结果，
   包括服务重启后；更改请求意图返回冲突。
4. 连接层只对首次成功创建启动既有 workspace setup，并发送 workspace.update。
   Directory 来源及独立 worktree 接口保持兼容；嵌套目录映射、配置复制和托管目录
   冲突处理复用既有 filesystem 实现，继续使用 Ait 的资源路径。
5. 接入 ADR-058 的后台命名：firstAgentContext 同时传递 prompt 和附件来源。
   显式 title 不建立自动命名资格；显式 branchName 不记录为可自动改名的占位分支。

## 范围

此决策覆盖本地 Git 新分支和已有分支 checkout。Forge PR checkout 和同请求创建 Agent
仍遵循既有不支持行为，返回明确错误，不静默退化成普通分支创建。
Setup 沿用既有后台执行机制；创建回执完成表示工作区已注册，不表示 setup 已完成。

## 验证

协议与服务单测验证字段、项目选择、保留 ID、title、分支优先级及错误。
真实 server/Git 回归验证嵌套 checkout、独立分支名、项目来源、setup、幂等与重启恢复。
执行结果和 Test coverage 记录在[验证报告](../../archive/reports/workspace/workspace-create-worktree.md)。
