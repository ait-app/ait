# ADR-069：活跃工作区的后台 Git fetch

- 状态：Accepted
- 日期：2026-10-02
- 关联：ADR-030、ADR-037、ADR-065

“从 main 更新”的合并行为已由 [ADR-083](adr-083-reset-workspace-to-origin-default.md) 中的工作区重置取代。

## 背景

Git 菜单与 Paseo 一致，但 Ait 的 Rust 服务没有刷新远端引用的后台任务。
因此“从 main 更新”可能使用过期的 `origin/main`，无限缓存的前端状态也无法发现远端提交。
用户要求补齐 Paseo 的后台刷新行为。对照版本为
[Paseo v0.10.2](https://github.com/getpaseo/paseo/releases/tag/v0.10.2)。

## 决策

1. metadata 声明 `WorkspaceGitObserver` / `WorkspaceGitObservation` 消费者端口，
   filesystem 实现仓库观察与 fetch。目录订阅的初始响应入队后才激活观察；
   后续快照更新符合过滤条件、未归档的工作区路径。订阅释放、断线或关闭时释放观察。
   单次目录读取不启动 fetch，metadata 不执行 Git，也不反向依赖 filesystem。
2. filesystem 按规范化的 `git-common-dir` 合并观察路径，同一仓库的多个 worktree 和客户端
   共享一个任务。首次观察立即 `git fetch origin --prune --quiet`，之后每 180 秒执行一次。
   每仓库不重叠，进程最多同时执行两个 fetch；使用独立预算和阻塞线程，不持有前台 Checkout 锁。
3. 非 Git、裸仓库和没有 origin 的目录跳过；每 180 秒重新发现仓库，以支持后来添加 origin。
   fetch 禁止终端凭据提示，最长 120 秒。最后一个观察者离开或进程关闭会取消并回收子进程；
   Unix 终止独立进程组，Windows 使用 taskkill。失败仅记录脱敏错误类别，下一轮重试。
   fetch 不合并、提交、推送或修改 HEAD、索引和工作区文件。
4. fetch 后重新读取仍被观察的 checkout 状态，通过共享 `SessionEvents` 发布去重的
   `checkout.status.update`。新 `checkout-git-events-v1` 标记只在生产者和目录服务均安装时发布；
   SDK adapter 据此声明 `checkout_status_update`，补齐其复用 RPC schema 所需的空 requestId。
   前端既有事件处理刷新状态缓存、提交列表和工作区比较。
5. 状态及 base diff 的未限定基准分支优先比较 origin 跟踪引用；完全限定引用保持精确语义。
   提交列表和“从 main 更新”使用本地与 origin 中进展更大的基准，沿用 Paseo 的选择规则。
   这样远端已获取而本地 main 尚未移动时，更新按钮仍能识别新提交，合并后也不会把远端提交
   误算成工作区自己的变更。“从 main 更新”本身继续只合并已有引用，不强制等待一次 fetch。

## 验证与边界

定向测试覆盖仓库去重、180 秒节奏、事件去重、失败重试、origin 新增、取消与任务回收、
并发预算、真实本机远端 fetch/prune、基准比较和 WebSocket 状态推送。
验证结果及覆盖率状态见[实施报告](../../reports/workspace/background-git-fetch.md)。

后台 fetch 不保证点击瞬间的远端状态；网络失败时保留上一次引用。需要严格最新版本时，
仍可显式 fetch。没有新增公共 RPC、持久化 schema、前端按钮或远端写入行为。

上游依据：
[观察生命周期及 180 秒调度](https://github.com/getpaseo/paseo/blob/v0.10.2/packages/server/src/server/workspace-git-service.ts#L2129)、
[fetch origin --prune](https://github.com/getpaseo/paseo/blob/v0.10.2/packages/server/src/server/workspace-git-fetch.ts)、
[基准引用选择和合并](https://github.com/getpaseo/paseo/blob/v0.10.2/packages/server/src/utils/checkout-git.ts)。
