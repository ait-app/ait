# ADR-217：事件驱动的 Workspace 运行时事实与低能耗 Git 读取

- 状态：已接受
- 日期：2026-11-07
- 关系：修订 [ADR-068](adr-068-workspace-runtime-summaries.md) 的刷新与唤醒规则；
  共享目录唤醒信号沿用 [ADR-210](../providers/adr-210-committed-runtime-read-snapshots.md) 的读视图边界。

## 背景

桌面端打开约 30 个 workspace 时，`aitd` 持续占用 20–28% CPU，每秒启动 45 个以上 `git` 进程，
并是 macOS 上能耗最高的进程。采样显示两个 `workspace-git-read` worker 始终满载：

- 每次侧边栏读取完成都唤醒共享的目录 `Changes`；Workspace 列表重新投影时把已过 2 秒 TTL 的
  checkout 再次排队，形成自我维持的循环。同一信号还让 Agent 目录完整重建、Hub 重新对账。
- 一次 `sidebar_summary` 经由完整 `status` 读取约启动 20 个 Git 进程。
- PATH 中的 `/usr/bin/git` 是 `xcrun` 垫片，每次调用先定位开发者工具链，短命令开销约增加 45%。
- 子进程以固定 10 ms 间隔轮询，网络命令每秒唤醒线程上百次。

## 决策

- **只在事实变化时唤醒**：Git/Forge 读取结果与缓存相同时不通知；连续未变化的读取按 cwd 指数
  退避（见 ADR-068 修订条目），变化后回到起始间隔。
- **Workspace 专用信号**：`model::changes::Changes::extend` 派生只扩展、不回传的信号。
  `Directory::with_changes` 持有一般信号并派生 `workspace_changes`；Workspace 目录订阅观察后者，
  `LocalWorkspaceRuntime` 只通知后者。登记表、终端和 Agent 变化仍通过一般信号唤醒所有投影；
  checkout 事实不再唤醒只读取 Agent 与 Hub 的投影。
- **文件事件失效（仅 macOS）**：`filesystem::workspace_runtime::watch` 惰性启动一个 `notify`
  `FSEvents` 流，递归覆盖已缓存 checkout 的工作树顶层、私有 Git 目录与共享 Git 目录，并合并
  嵌套路径。相关事件把条目标记为 dirty，满足 2 秒最短间隔且仍有读者时立即排队，否则由下一次
  目录读取接纳；被丢弃或无法启动的排队读取保留 dirty。对象库、reflog 与其他 worktree 的私有状态
  不触发；链接 worktree 只关注共享目录中的 `refs`、`packed-refs`、`config`、`reftable` 与 `info`。
  监视集合在监视状态锁内按缓存计算并批量提交，每次最多重启一次事件流；重启后所有被覆盖条目
  标记 dirty，补上重启间隙与首次监视前的变化。只有全部根路径都被成功监视的条目才把轮询上限
  放宽到 5 分钟，仅作为丢失事件的兜底；其余条目和其他平台保持 30 秒上限，避免 Linux `inotify`
  为大型目录树逐目录建立 watch。监视器在独立线程上停止，避免在事件线程释放最后一个运行时
  句柄时自等待。侧边栏读取全部使用 `--no-optional-locks`，自身不写入仓库，不会自我触发事件。
- **合并 Git 读取**：侧边栏摘要改为约 7 个命令：一次 `rev-parse` 取得布局，一次
  `status --porcelain=v2 --branch -z --untracked-files=all` 取得 HEAD、分支、上游计数、
  脏状态与未跟踪文件，一次 `remote -v`，一次 `for-each-ref` 取得默认分支相关引用，然后是基准比较、
  merge-base 与 `diff --numstat`。未跟踪文件统计仍只计 cwd 之下的文件，并从工作树顶层解析路径。
  默认分支与比较基准规则与完整 `status` 共用同一实现；悬空的
  `origin/HEAD` 仍由 `symbolic-ref` 解析；状态输出超限时回退到完整 `status` 且不给出行数统计。
- **Git 可执行文件**：macOS 上 PATH 首个 `git` 是 `/usr/bin/git` 时，用 `xcrun --find git` 解析一次
  真实路径并复用；文件消失时回退到 PATH 查找。其他 PATH 条目和其他平台不变。
- **子进程等待**：从 4 ms 开始倍增，最长 50 ms；终止宽限期仍按 10 ms 检查。

## 后果与验证

空闲 checkout 在 macOS 上基本不再启动 Git；仍在变化的 checkout 每 2 秒左右读取一次，每次约 7 个
命令。代价：错过的文件事件最多 5 分钟后才被轮询发现；构建产生的大量文件事件会让对应 checkout
以基准间隔刷新；Forge 仍按 ADR-068 轮询网络。新增 macOS 专用依赖 `notify`（`macos_fsevent`）。

测试覆盖：路径相关性、递归覆盖合并与逐条目覆盖判定、丢弃读取保留 dirty、子目录 workspace 的
未跟踪统计、真实 `FSEvents` 在不轮询时刷新空闲 checkout、dirty 状态的
接纳间隔、侧边栏与完整 `status` 在多远程、`insteadOf`、上游、`origin/HEAD`（含悬空）、分离 HEAD
与托管 worktree 下逐字段一致、porcelain v2 解析、扩展信号单向传播、Git 可执行文件解析与子进程
退避轮询。
