# ADR-092：工作区重置同步 origin 同名分支

- 状态：Accepted
- 日期：2026-10-07
- 关联：修订 ADR-083 的远端分支行为；沿用 ADR-069、ADR-084

## 背景

工作区重置会把本地初始分支恢复到 origin 的最新默认分支，但已推送的同名远端分支仍指向旧提交。后续工作区状态比较和 PR 会继续包含已丢弃的提交。

## 决策

1. 沿用 ADR-083 的托管 worktree 校验、初始分支恢复、默认分支 fetch 和本地 hard reset。Git 操作继续由 filesystem 的本地 checkout adapter 实现，不增加协议字段或跨域依赖。
2. 在本地变更前，通过同一次 `git ls-remote --symref origin HEAD refs/heads/<initialBranch>` 查询 origin 的默认分支与初始分支当前提交。只认实际远端引用，不依赖可能残留或缺失的本地远端跟踪引用。查询或 fetch 失败时，本地分支和 HEAD 保持原状。
3. 本地 reset 成功后，若查询时存在 `refs/heads/<initialBranch>`，执行一次明确指定 `HEAD:refs/heads/<initialBranch>` 的非交互 push。使用 `--force-with-lease=refs/heads/<initialBranch>:<observedCommit>` 强制更新到本地重置后的同一提交，并使用 `--no-follow-tags` 避免附带本地标签。目标固定为 origin 的初始分支，不受 upstream 或普通 push 配置的分支选择影响。
4. 查询时不存在同名远端分支则跳过 push，不创建远端分支。远端在查询后发生变化或被删除时，lease 拒绝更新；不覆盖并发提交或重新创建刚删除的分支。
5. Push 被权限、分支保护、网络或 lease 校验拒绝时，请求返回失败，错误明确说明本地已重置、远端重置未完成。本地 reset 不回滚。重试会重新查询远端；只有完整操作成功后才按现有服务逻辑更新持久化工作区分支字段。
6. 所有支持语言的重置确认提示说明：存在 origin 同名分支时会强制推送并重置到同一提交。

## 后果与验证

用户确认的重置操作同时丢弃同名远端特性分支的旧提交。Git 无法原子完成本地 reset 与远端 push，因此失败提示保留部分完成状态。

使用隔离的临时本地仓库和 bare origin 验证 main/master、初始分支已存在或当前分支已改名、远端跟踪引用缺失或残留、非快进强制更新、推送拒绝及重试。Unix hook 测试验证一次 push、仅更新初始分支、不附带标签，以及查询后远端更新或删除时拒绝覆盖。原有 worktree 占用和非托管目录校验继续保留。
