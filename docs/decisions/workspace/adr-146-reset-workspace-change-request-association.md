# ADR-146：Workspace 重置清除变更请求关联

- 状态：已接受
- 日期：2026-10-13

## 背景

Git 分支改名保留 upstream 和 fork pushRemote，Forge 也可能按同名分支返回旧 PR/MR。仅刷新界面不能清除重置后的关联。

## 决策

Filesystem 的本地 Git adapter 在 managed workspace 重置及必要的远程推送成功后，写入本地 Git 配置 `branch.<initial-branch>.ait-reset-at`（UTC RFC 3339），并清除该分支的 `remote`、`merge`、`pushRemote` 和 `gh-merge-base`。

本地 Forge adapter 读取当前分支的重置时间。GitHub 使用 `createdAt`，GitLab 使用 `created_at`，不再将创建时间早于或等于重置时间的 PR/MR 投影为当前 workspace 的关联；重置后新建的请求正常显示。存在重置标记但缺失或无法解析创建时间时，不恢复旧关联。没有标记的分支保持原行为。

该配置只属于本地 checkout/Forge adapter 协作，不进入 domain registry 或 wire schema。侧边栏和 PR 面板复用同一 Forge 读取规则，daemon 重启后规则仍然有效。Git 配置中的时间随分支改名保留。不会关闭或删除远端 PR/MR，也不清理聊天附件。

## 影响与限制

重置后的下一次普通 push 按初始分支重新建立远程关系，不再使用旧 PR fork 的推送目标。时间比较依赖本机与 Forge 的时钟一致性；Forge 秒级时间戳落在重置同一秒时按旧关联处理。
