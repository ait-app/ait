# ADR-138：定时任务固定归属于 Workspace

- 状态：接受
- 日期：2026-10-12
- 修订：[ADR-047](../daemon/adr-047-daemon-schedule-browser.md) 中新 Agent 任务每次创建独立 Workspace 的执行方式；仅旧版未绑定任务保留原行为。

## 背景

原定时任务只有 Agent 目标或包含 `cwd` 的新 Agent 配置，没有任务所属 Workspace。
创建界面只提供新 Agent 配置，已有 Agent 任务只能修改周期。新 Agent 执行还创建并清理
一个独立 Workspace，因此无法从已有 Workspace 创建和管理属于它的任务。

## 决策

1. 新建任务必须提供 `workspaceId`。任务拥有固定的 Workspace 身份，不能移动到另一个
   Workspace；目标可以在未运行时切换为该 Workspace 内未归档的 Agent，或每次新建 Agent。
   `run.workspaceId` 记录实际执行位置，不再表示可以无条件清理的资源。
2. `domain::schedule` 持有记录、状态和触发值；`schedule` 继续只依赖 domain/model，协调
   原子持久化、触发、次数限制和运行历史。通过 `Runner::resolve` 请求宿主校验任务归属和
   Agent 成员关系，并规范化新 Agent 配置中的目录。daemon 使用 metadata/provider 完成校验，
   执行前再次检查目标。任务模块不直接访问 Workspace 或 Agent registry。
3. 已绑定的新 Agent 任务在现有 Workspace 的实际目录中创建 Agent，不创建 Workspace 或
   Worktree。`archiveOnFinish` 只控制该次新建 Agent 的归档，默认开启；正常结束、失败清理和
   重启恢复都不能归档任务所属 Workspace。需要 Worktree 时，先在该 Worktree Workspace 创建任务。
4. 触发支持 `once`（绝对时间）、`every`（真实毫秒间隔）和五字段 `cron`（IANA 时区）。
   周期任务可设 `startAt`：间隔任务在指定时间首次执行，Cron 取不早于该时间的下一匹配点。
   UI 提供间隔和 Cron，运行次数默认 1 来表达单次，不提供独立的单次选项。底层 `once`
   保留读取、执行和编辑兼容。UI 默认不立即运行，保留显式 `runOnCreate` 的接口兼容行为。
   暂停只阻止后续自动触发。
5. `maxRuns` 统计已结算的成功、失败和手动运行；`expiresAt` 同时约束自动和手动准入。
   单次任务结算后完成，包括失败与重启时恢复的中断运行。错过但未接纳的单次任务在启动后
   执行一次；周期任务跳过历史触发点。完成状态保持终态。
6. 校验在独立、有界异步任务中完成，不占用持久化 actor 的等待路径。每次目标修改校验
   保存原记录版本，提交前检查 `updatedAt`，并重新应用业务校验；并发修改或运行期间的目标
   修改返回冲突。暂停和非目标字段修改仍可在运行中执行；删除等待本次运行结算。
7. `schedule.list.request` 可按 `workspaceId` 过滤。Workspace 新建菜单在拉取请求后增加
   定时任务，打开按下一次触发时间排序、按日期分组的列表，点击条目直接编辑或新建。
   保存与取消返回列表；通过条目的历史入口查看详情。列表筛选、创建、编辑和详情导航
   持久化在 Workspace 布局中。表单统一支持两个目标，以及名称、提示词、时间规则、
   截止时间和次数。单个可搜索的执行 Agent 选择器提供新建入口和已有 Agent 候选；
   选择现存 Agent 直接绑定，选择新建才显示创建配置。详情保留运行状态、输出、Agent 跳转、
   手动运行、暂停/恢复和删除。
   全局 Schedules 创建入口依次选择 Host、Project、Workspace，再使用同一表单。
   Project 和 Workspace 按主机身份及 `projectId` 筛选；更换上级选择清空下级和任务草稿。
   已绑定任务的编辑保留固定归属。
8. 文件适配器读取版本 1 和 2，写入版本 2。旧记录缺失 `workspaceId` 时保留 null，不根据
   `cwd` 猜测归属；同一路径可能对应多个 Workspace。旧任务仍可从全局任务页管理并按原方式
   执行。编辑时明确选择 Workspace 后绑定一次；已有历史中的执行 Workspace 身份保留。
   新版创建 RPC 拒绝缺失归属的请求，旧调用方需补充 `workspaceId`。

## 后果

任务的生命周期与 Workspace 身份一致，路径变化由宿主执行时读取；从表单传入的 `cwd`
不会改变已绑定任务的位置。关闭标签页不删除任务，关闭客户端也不会停止仍在线 daemon
中的调度。守护进程离线期间不执行任务；重启恢复与暂停状态按上述规则处理。

旧任务的兼容读取与执行是明确的过渡分支，新任务不会进入该分支。存储版本升级后，旧版
仅支持版本 1 的 daemon 不能读取已写入的版本 2 文件，回退前需恢复对应备份。
