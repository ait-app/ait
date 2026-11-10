---
id: ADR-066
status: accepted
date: 2026-09-29
paths:
  - crates/filesystem
---

# ADR-066：通过 glab 支持 GitLab Forge

## 背景

Desktop 0.0.8 的 PR 面板会对 GitLab origin 调用 `gh pr view`，并把 GitLab 域名写入
`GH_HOST`。GitHub CLI 发出的 `PullRequestForBranch` GraphQL 查询包含 GitLab 不支持的
`PullRequestState` 和 `Query.repository`，因此状态刷新持续报错。

本地 Paseo `30178c4f58b67f8472901356e1484022bd835de0` 已有 Forge resolver、CLI
认证探测、GitLab service 和前端策略。Ait 复用其设计与协议投影，在现有 Rust adapter 内实现。

## 决策

- `filesystem::local::forge` 仍实现现有 `ForgeRuntime` port；增加 GitLab 子模块和
  host resolver，不新增业务能力组，不把 CLI 或平台规则移入 domain。
- `github.com`、`*.ghe.com` 与 `gitlab.com` 直接选择 adapter。SSH alias 先执行
  `ssh -G` 取得实际 hostname；未知域名依次运行 `gh/glab auth status --hostname`，
  使用已有登录配置识别自建实例。探测失败不再默认按 GitHub 处理。
- 探测每个 CLI 最多 5 秒；进程内缓存最多 128 个 host，失败结果 30 秒后重试。
  成功结果保留到 server 重启。每次重新读取 origin，以支持仓库 remote 变更。
- GitLab 使用 `glab` 的 MR、issue、CI 和 REST 命令；显式限定 host/project，保留多层
  group 路径并对 REST project ID 编码。沿用现有进程超时、输出限制和错误分类；用户输入
  通过独立 argv 传递，不拼接 shell。认证由 Host 上的 CLI 管理，不另存 token。
- MR 状态、搜索、创建、合并、自动合并、讨论与流水线投影到现有中立协议。
  同名分支的历史已关闭 MR 必须匹配 HEAD SHA；pipeline aggregate 不被可选 manual job
  覆盖。自动合并只在 pipeline 活跃时开启，取消使用 REST 专用接口；直接合并显式设置
  `--auto-merge=false`，并在后端重新校验合并状态。
- 复用 `ServerInfo.features` 中的 `forge-gitlab-v1` 公布 GitLab adapter，与当前用户
  是否登录分开。旧 server 缺少此标记时保持旧行为。Desktop adapter 据此开启现有 GitLab
  展示，并依据已实现 RPC 公布 check details / auto-merge 能力。
- 已识别 GitLab 后的状态查询错误携带 forge 身份，避免错误面板回退成 GitHub。
- 复用 Paseo 的 MR checkout refs：`origin` 上的 `refs/merge-requests/<iid>/head` 优先，
  源分支 ref 回退。`ChangeRequestCheckout` port 携带有序 remote/ref，Git adapter 验证
  ref 后执行 fetch；GitHub 保持 origin/upstream PR ref 顺序。跨项目 MR 标记为不可信
  setup 来源，不为目标仓库分支设置错误的 upstream/push remote。

## 使用与后果

在运行 Ait server 的 Host 上安装 `glab`，并执行 `glab auth login --hostname <host>`。
公共与自建 GitLab 均使用这个登录来源。自建 host 未登录或 CLI 不可用时无法完成平台识别，
返回 `no_remote`；完成登录后等待失败缓存过期或重启 server。已识别的平台可以显示
`cli_missing` / `unauthenticated` 状态。

升级 server 可以修复旧 Desktop 的错误路由；完整 GitLab 流水线面板和能力协商需要本次
Desktop adapter 一起发布。没有修改 0.0.8 安装包，也没有新增 GitLab 项目浏览/克隆入口；
现有 GitHub 专用项目目录 API 保持其平台范围。从 MR 创建 worktree 走现有创建流程，
支持 MR ref、源分支回退、分支重名和 fork setup 授权保护。

讨论最多读取 100 个 discussion，并探测下一项以准确报告截断。自建实例的 API 协议与
端口沿用 glab 配置；本次测试覆盖标准 HTTPS remote 和 SSH alias，不宣称覆盖所有自定义
端口、HTTP-only、代理或 SSO 部署。

验证范围与 Test coverage 见[实施报告](../../archive/reports/workspace/gitlab-forge.md)。
