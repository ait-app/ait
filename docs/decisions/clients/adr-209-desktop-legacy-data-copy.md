# ADR-209：桌面启动时引导复制旧数据并恢复异常退出的 daemon

- 状态：Accepted
- 日期：2026-11-02
- 关联：ADR-053、ADR-055、ADR-128、GitHub Issue #557

## 背景

独立 daemon 默认使用 `~/.ait-server`，桌面内置 daemon 使用
`~/.ait-server-desktop`。桌面切换到独立数据目录时保留旧数据但不自动迁移；已有用户可能
继续通过旧 Host 使用 `~/.ait-server`。当桌面内置 daemon 后来恢复运行时，客户端会注册一
个新的空 Host，使仍在旧目录中的项目和会话看起来像被删除。

桌面保留 daemon 的连接记录原先只在正常退出时写入。Electron 被终止时没有交接记录，下一
次启动会尝试为同一数据目录创建第二个 daemon，触发实例锁冲突。连续启动请求会重复产生相
同错误。

## 决策

1. 仅在没有显式 `AIT_SERVER_DATA_DIR`、且桌面内置 daemon 尚未启动时检查默认目录。若
   `~/.ait-server` 的 Agent、Project 或 Workspace registry 有记录，而
   `~/.ait-server-desktop` 对应 registry 为空，桌面必须先显示复制确认，不能静默启动空
   Host。
2. 用户取消时保持两个目录不变并阻止空 Host 启动。用户确认后，桌面调用随包 daemon 的
   `migrate-data` 命令；复制和锁语义不在 renderer 或 shell 中实现。如果源或目标目录存在
   可通过私有 token、PID 和 Server ID 再次核验的桌面保留 daemon，确认按钮明确显示“停止
   旧进程并复制”，且只有用户点击后才停止它。
3. 迁移命令按规范路径顺序同时取得源、目标数据目录的实例租约。任一 daemon 仍在运行、源
   目录没有记录或目标已有记录时，迁移失败且不覆盖目标。
4. 迁移只发布 `agents/` 与 `projects/`，涵盖 Agent registry、Timeline SQLite、Project、
   Workspace、标签和项目图标。旧目录不删除；其中已有 Workspace 的绝对路径继续有效。旧
   Worktree 留在原位置，目标通过私有的旧根目录映射继续将其识别为托管 Worktree；新
   Worktree 仍创建在目标目录中。
5. 复制先进入目标同一文件系统的 staging 目录。发布前把目标原 `agents/` 和 `projects/`
   移入永久备份；发布后重新读取三个 registry 并统计 Timeline 行数，只有与源摘要一致才
   成功。目标保留自己的 `server-id`、本地 API token、在线服务凭据、配置、日志和运行态。
   目标、staging 和备份目录在 Unix 上固定为 `0700`，迁移状态文件固定为 `0600`。
6. 发布前在目标内持久化迁移 journal。异常终止后，普通 daemon 启动必须拒绝使用半发布
   数据；桌面和 `migrate-data` 重试先根据 journal 回滚，再重新执行完整复制。journal 只在
   发布、校验和目录同步全部成功后删除。
7. daemon 每次通过认证就绪检查后立即写入私有 `desktop-daemon.json`，不再等正常退出。
   后续桌面验证 token、PID 和 Server ID 后认领同一 daemon，并核对 Instance ID。已记录进程
   存活但暂时无法验证时停止启动，不创建竞争写入者；同一 PID、token 和 Server ID 下的
   Instance ID 换代视为进程内重启并更新记录。确认进程退出或 PID／Server ID 不符后才清理
   记录。启动和重启入口都必须先经过数据目录准备门。
8. 停止迁移阻塞进程时先重新核对确认框中显示的 PID、启动时间和 Server ID，再请求认证的
   `server.shutdown`。超时后仅在 token 仍能证明 PID 未变时依次发送 `SIGTERM` 和
   `SIGKILL`。记录无法认证、身份在确认后变化或无法证明仍为同一进程时，不显示或不执行
   强制停止，迁移继续按持锁失败处理。

## 后果

- 首次遇到旧数据的用户得到明确、可取消的复制入口；复制失败不会进入空 Host。
- 活跃旧 daemon 必须先停止。桌面可在用户明确确认后停止已认证的旧进程；无法认证的进程只
  会解释失败并允许重试，不删除锁，也不强制终止未知进程。
- 迁移后的新 daemon 使用新的 Host 身份；旧 Host 和源目录保留，便于回滚。旧目录不能在
  用户确认不再需要旧 Worktree 和备份前删除。
- 本次不合并两个非空目录，不迁移在线服务身份，也不把旧目录自动改名或删除。

## 验证

- Rust 单元测试覆盖成功迁移、目标非空拒绝、源持锁拒绝、目标身份保留和备份保留。
- Rust 单元与进程测试覆盖中断 journal 恢复、普通启动拒绝半发布数据、私有目录权限和旧
  Worktree 根继续识别。
- daemon 真实进程测试覆盖 CLI 参数、复制结果、摘要和备份路径。
- Desktop 单元测试覆盖候选检测、目标非空跳过、用户确认和取消不写入。
- Desktop 真实 Rust 子进程测试覆盖未正常交接时按 PID、Server ID 和 Instance ID 重新认领。
- Desktop 进程测试覆盖迁移确认后通过认证关闭保留 daemon，以及认证失败时不提供强制停止。

## Test coverage

未测量覆盖率；本次按仓库规范执行受影响的 Rust 和 TypeScript 定向测试，不扩大到 workspace
全量覆盖率。CI 应保留对应测试报告，若发布前要求覆盖率，再对 `daemon` binary 与桌面 daemon
管理模块执行限定文件测量。
